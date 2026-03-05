use ratatui::prelude::*;
use ratatui::symbols::border;

use crate::models::asset::AssetCategory;

/// Shadow opacity — controls how dark the popup shadow is.
/// 0.0 = fully transparent (no shadow), 1.0 = fully black.
const SHADOW_OPACITY: f32 = 0.70;

// ---- Border set constants ----

/// Active/focused panel border: double-line top (═══) with single-line sides (│).
/// Gives a premium, Bloomberg-like feel to the focused panel.
pub const BORDER_ACTIVE: border::Set = border::Set {
    top_left: "╒",
    top_right: "╕",
    bottom_left: "└",
    bottom_right: "┘",
    vertical_left: "│",
    vertical_right: "│",
    horizontal_top: "═",
    horizontal_bottom: "─",
};

/// Inactive/unfocused panel border: standard single-line (┌───┐).
/// Clean and minimal, recedes visually behind the active panel.
pub const BORDER_INACTIVE: border::Set = border::Set {
    top_left: "┌",
    top_right: "┐",
    bottom_left: "└",
    bottom_right: "┘",
    vertical_left: "│",
    vertical_right: "│",
    horizontal_top: "─",
    horizontal_bottom: "─",
};

/// Popup/overlay border: full double-line (╔═══╗ / ║ ║ / ╚═══╝).
/// Maximum visual weight for modals that need to stand out above everything.
pub const BORDER_POPUP: border::Set = border::DOUBLE;

// ---- Animation constants ----
pub const PULSE_PERIOD: u64 = 90; // ~1.4s at 60fps
pub const FLASH_DURATION: u64 = 45; // ~0.7s at 60fps
pub const PULSE_PERIOD_BORDER: u64 = 120; // 2s at 60fps — subtle breathing for active panel borders
pub const SELECTION_FLASH_DURATION: u64 = 15; // ~0.25s at 60fps — row highlight on selection change
pub const THEME_TOAST_DURATION: u64 = 90; // ~1.5s at 60fps — theme name toast on cycle
pub const SORT_FLASH_DURATION: u64 = 30; // ~0.5s at 60fps — sort indicator flash on change

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
    "inferno",
    "neon",
    "hacker",
    "pastel",
    "miasma",
];

pub fn theme_by_name(name: &str) -> Theme {
    match name {
        "catppuccin" => catppuccin(),
        "nord" => nord(),
        "dracula" => dracula(),
        "solarized" => solarized(),
        "gruvbox" => gruvbox(),
        "inferno" => inferno(),
        "neon" => neon(),
        "hacker" => hacker(),
        "pastel" => pastel(),
        "miasma" => miasma(),
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

/// Nord — cool arctic blue-gray with enhanced contrast.
/// Darker surfaces for depth, brighter accents for readability.
pub fn nord() -> Theme {
    Theme {
        name: "nord",
        surface_0: Color::Rgb(36, 40, 52),       // darker base for more depth
        surface_1: Color::Rgb(46, 52, 64),        // original base becomes panel interior
        surface_1_alt: Color::Rgb(50, 56, 70),    // subtle stripe
        surface_2: Color::Rgb(59, 66, 82),        // header/status
        surface_3: Color::Rgb(76, 86, 106),       // selected row — brighter jump
        border_active: Color::Rgb(136, 192, 208), // nord frost cyan — keep
        border_inactive: Color::Rgb(67, 76, 94),  // slightly dimmer for contrast
        border_accent: Color::Rgb(163, 190, 240), // brighter blue accent
        border_subtle: Color::Rgb(50, 56, 70),
        text_primary: Color::Rgb(242, 244, 248),  // brighter white for contrast
        text_secondary: Color::Rgb(200, 208, 220),// clearer secondary
        text_muted: Color::Rgb(110, 120, 140),    // more visible muted
        text_accent: Color::Rgb(136, 192, 208),
        gain_green: Color::Rgb(145, 210, 130),    // more saturated, brighter green
        loss_red: Color::Rgb(210, 85, 95),        // more vivid red
        neutral: Color::Rgb(200, 208, 220),
        live_green: Color::Rgb(145, 210, 130),
        stale_yellow: Color::Rgb(240, 210, 130),  // warmer, brighter
        key_hint: Color::Rgb(240, 210, 130),
        chart_line: Color::Rgb(136, 192, 208),
        chart_grad_low: Color::Rgb(210, 85, 95),
        chart_grad_mid: Color::Rgb(240, 210, 130),
        chart_grad_high: Color::Rgb(145, 210, 130),
        cat_equity: Color::Rgb(129, 161, 193),
        cat_crypto: Color::Rgb(220, 140, 100),    // warmer, more saturated
        cat_forex: Color::Rgb(145, 210, 130),
        cat_commodity: Color::Rgb(240, 210, 130),
        cat_fund: Color::Rgb(190, 140, 185),      // more vivid purple
        cat_cash: Color::Rgb(200, 208, 220),
    }
}

/// Dracula — deep purple/dark with vivid, punchy accents.
/// Leans hard into purples, magentas, and cyans. Unapologetically bold.
pub fn dracula() -> Theme {
    Theme {
        name: "dracula",
        surface_0: Color::Rgb(30, 28, 44),       // deeper, more purple-tinted base
        surface_1: Color::Rgb(40, 38, 58),        // purple-dark panel
        surface_1_alt: Color::Rgb(36, 34, 52),    // stripe
        surface_2: Color::Rgb(50, 46, 70),        // richer purple header
        surface_3: Color::Rgb(65, 58, 90),        // vivid purple selection
        border_active: Color::Rgb(200, 150, 255), // brighter purple
        border_inactive: Color::Rgb(90, 80, 140), // purple-tinted inactive
        border_accent: Color::Rgb(255, 100, 200), // hot pink accent
        border_subtle: Color::Rgb(50, 46, 70),
        text_primary: Color::Rgb(250, 250, 245),
        text_secondary: Color::Rgb(180, 175, 210),// purple-tinted secondary
        text_muted: Color::Rgb(100, 95, 140),     // purple muted
        text_accent: Color::Rgb(200, 150, 255),   // bright purple
        gain_green: Color::Rgb(80, 255, 130),     // punchy green
        loss_red: Color::Rgb(255, 70, 80),        // vivid red
        neutral: Color::Rgb(180, 175, 210),
        live_green: Color::Rgb(80, 255, 130),
        stale_yellow: Color::Rgb(245, 250, 140),
        key_hint: Color::Rgb(245, 250, 140),
        chart_line: Color::Rgb(130, 230, 255),    // bright cyan
        chart_grad_low: Color::Rgb(255, 70, 80),
        chart_grad_mid: Color::Rgb(245, 250, 140),
        chart_grad_high: Color::Rgb(80, 255, 130),
        cat_equity: Color::Rgb(130, 230, 255),    // bright cyan
        cat_crypto: Color::Rgb(255, 170, 100),    // warm orange
        cat_forex: Color::Rgb(80, 255, 130),      // punchy green
        cat_commodity: Color::Rgb(245, 250, 140), // electric yellow
        cat_fund: Color::Rgb(200, 150, 255),      // bright purple
        cat_cash: Color::Rgb(180, 175, 210),
    }
}

/// Solarized Dark — teal-tinted dark with warm/cool accent balance.
/// Enhanced contrast and brighter accents while preserving the distinctive teal feel.
pub fn solarized() -> Theme {
    Theme {
        name: "solarized",
        surface_0: Color::Rgb(0, 36, 46),        // slightly darker base for depth
        surface_1: Color::Rgb(0, 43, 54),         // canonical solarized base03
        surface_1_alt: Color::Rgb(3, 48, 60),     // subtle stripe
        surface_2: Color::Rgb(7, 54, 66),          // canonical base02
        surface_3: Color::Rgb(20, 72, 85),         // brighter selection
        border_active: Color::Rgb(42, 161, 232),   // brighter blue for punch
        border_inactive: Color::Rgb(75, 100, 108), // slightly dimmer
        border_accent: Color::Rgb(120, 120, 210),  // brighter violet
        border_subtle: Color::Rgb(7, 54, 66),
        text_primary: Color::Rgb(245, 240, 225),   // slightly brighter
        text_secondary: Color::Rgb(160, 175, 175), // clearer
        text_muted: Color::Rgb(88, 110, 117),
        text_accent: Color::Rgb(42, 161, 232),
        gain_green: Color::Rgb(150, 175, 0),       // brighter, more saturated
        loss_red: Color::Rgb(235, 55, 52),          // slightly brighter
        neutral: Color::Rgb(160, 175, 175),
        live_green: Color::Rgb(150, 175, 0),
        stale_yellow: Color::Rgb(200, 150, 0),     // richer gold
        key_hint: Color::Rgb(200, 150, 0),
        chart_line: Color::Rgb(42, 161, 232),
        chart_grad_low: Color::Rgb(235, 55, 52),
        chart_grad_mid: Color::Rgb(200, 150, 0),
        chart_grad_high: Color::Rgb(150, 175, 0),
        cat_equity: Color::Rgb(42, 161, 232),
        cat_crypto: Color::Rgb(215, 85, 30),       // brighter orange
        cat_forex: Color::Rgb(150, 175, 0),
        cat_commodity: Color::Rgb(200, 150, 0),
        cat_fund: Color::Rgb(120, 120, 210),
        cat_cash: Color::Rgb(160, 175, 175),
    }
}

/// Gruvbox Dark — warm retro palette with bright, saturated accents.
/// Leans into the classic Gruvbox identity: aqua, lime, orange, pink on warm dark.
/// Distinct from Miasma: Gruvbox is retro-bright, Miasma is atmospheric/smoky.
pub fn gruvbox() -> Theme {
    Theme {
        name: "gruvbox",
        surface_0: Color::Rgb(28, 28, 28),       // gruvbox hard dark bg
        surface_1: Color::Rgb(40, 40, 40),        // standard bg
        surface_1_alt: Color::Rgb(36, 36, 36),    // stripe
        surface_2: Color::Rgb(50, 48, 47),        // header
        surface_3: Color::Rgb(70, 65, 60),        // selection
        border_active: Color::Rgb(250, 189, 47),  // bright yellow — signature gruvbox
        border_inactive: Color::Rgb(90, 82, 75),  // dimmer
        border_accent: Color::Rgb(254, 128, 25),  // bright orange accent
        border_subtle: Color::Rgb(50, 48, 47),
        text_primary: Color::Rgb(251, 241, 199),  // gruvbox fg0 (bright cream)
        text_secondary: Color::Rgb(213, 196, 161),// gruvbox fg2
        text_muted: Color::Rgb(124, 111, 100),    // gruvbox gray
        text_accent: Color::Rgb(250, 189, 47),    // bright yellow
        gain_green: Color::Rgb(184, 187, 38),     // gruvbox bright green
        loss_red: Color::Rgb(251, 73, 52),        // gruvbox bright red
        neutral: Color::Rgb(213, 196, 161),
        live_green: Color::Rgb(184, 187, 38),
        stale_yellow: Color::Rgb(250, 189, 47),
        key_hint: Color::Rgb(250, 189, 47),
        chart_line: Color::Rgb(142, 192, 124),    // gruvbox bright aqua-green
        chart_grad_low: Color::Rgb(251, 73, 52),
        chart_grad_mid: Color::Rgb(250, 189, 47),
        chart_grad_high: Color::Rgb(184, 187, 38),
        cat_equity: Color::Rgb(131, 165, 152),    // gruvbox aqua
        cat_crypto: Color::Rgb(254, 128, 25),     // bright orange
        cat_forex: Color::Rgb(184, 187, 38),      // bright green
        cat_commodity: Color::Rgb(250, 189, 47),  // bright yellow
        cat_fund: Color::Rgb(211, 134, 155),      // gruvbox pink
        cat_cash: Color::Rgb(213, 196, 161),      // warm cream
    }
}

/// Inferno — deep blacks with fire reds, oranges, and amber accents.
/// Gains glow hot, losses smolder like dying embers.
pub fn inferno() -> Theme {
    Theme {
        name: "inferno",
        surface_0: Color::Rgb(10, 8, 6),       // near-black with warm undertone
        surface_1: Color::Rgb(20, 14, 10),      // very dark brown
        surface_1_alt: Color::Rgb(25, 17, 12),  // slightly lighter for striping
        surface_2: Color::Rgb(35, 22, 14),      // dark ember
        surface_3: Color::Rgb(55, 30, 18),      // smoldering coal
        border_active: Color::Rgb(235, 140, 40),   // bright amber
        border_inactive: Color::Rgb(80, 45, 25),   // dim ember
        border_accent: Color::Rgb(255, 180, 50),   // hot gold
        border_subtle: Color::Rgb(45, 28, 16),     // barely visible warmth
        text_primary: Color::Rgb(245, 230, 210),   // warm white
        text_secondary: Color::Rgb(180, 150, 120), // warm gray
        text_muted: Color::Rgb(100, 75, 55),       // dark warm gray
        text_accent: Color::Rgb(255, 160, 40),     // fire orange
        gain_green: Color::Rgb(255, 180, 30),      // golden glow (gains = hot)
        loss_red: Color::Rgb(180, 40, 30),         // smoldering red
        neutral: Color::Rgb(180, 150, 120),
        live_green: Color::Rgb(255, 180, 30),
        stale_yellow: Color::Rgb(200, 130, 40),
        key_hint: Color::Rgb(255, 200, 80),
        chart_line: Color::Rgb(245, 130, 35),      // flame orange
        chart_grad_low: Color::Rgb(120, 20, 15),   // deep ember
        chart_grad_mid: Color::Rgb(235, 140, 40),  // amber
        chart_grad_high: Color::Rgb(255, 220, 60), // white-hot gold
        cat_equity: Color::Rgb(235, 130, 40),      // amber
        cat_crypto: Color::Rgb(255, 80, 30),       // hot red-orange
        cat_forex: Color::Rgb(255, 200, 60),       // gold
        cat_commodity: Color::Rgb(200, 100, 30),   // copper
        cat_fund: Color::Rgb(180, 70, 50),         // dark flame
        cat_cash: Color::Rgb(180, 160, 130),       // warm ash
    }
}

/// Neon — cyberpunk-inspired. Electric pink, cyan, purple on dark.
/// Think synthwave, retrowave, Blade Runner.
pub fn neon() -> Theme {
    Theme {
        name: "neon",
        surface_0: Color::Rgb(8, 8, 18),        // deep void blue-black
        surface_1: Color::Rgb(14, 14, 28),       // dark navy
        surface_1_alt: Color::Rgb(18, 16, 34),   // slight purple tint
        surface_2: Color::Rgb(24, 20, 42),       // dark purple
        surface_3: Color::Rgb(38, 30, 60),       // medium purple
        border_active: Color::Rgb(255, 50, 180),    // hot pink
        border_inactive: Color::Rgb(60, 40, 90),    // muted purple
        border_accent: Color::Rgb(0, 230, 255),     // electric cyan
        border_subtle: Color::Rgb(30, 25, 50),      // subtle purple
        text_primary: Color::Rgb(235, 235, 255),    // cool white
        text_secondary: Color::Rgb(160, 150, 200),  // lavender
        text_muted: Color::Rgb(80, 70, 120),        // dim purple
        text_accent: Color::Rgb(0, 230, 255),       // cyan
        gain_green: Color::Rgb(0, 255, 160),        // neon green-cyan
        loss_red: Color::Rgb(255, 50, 100),         // hot pink-red
        neutral: Color::Rgb(160, 150, 200),
        live_green: Color::Rgb(0, 255, 160),
        stale_yellow: Color::Rgb(255, 220, 50),
        key_hint: Color::Rgb(255, 220, 50),
        chart_line: Color::Rgb(0, 200, 255),         // bright cyan
        chart_grad_low: Color::Rgb(255, 30, 80),     // hot pink
        chart_grad_mid: Color::Rgb(180, 50, 255),    // electric purple
        chart_grad_high: Color::Rgb(0, 255, 160),    // neon green
        cat_equity: Color::Rgb(0, 180, 255),         // sky cyan
        cat_crypto: Color::Rgb(255, 50, 180),        // hot pink
        cat_forex: Color::Rgb(0, 255, 160),          // neon green
        cat_commodity: Color::Rgb(255, 220, 50),     // electric yellow
        cat_fund: Color::Rgb(180, 50, 255),          // purple
        cat_cash: Color::Rgb(160, 160, 200),         // muted lavender
    }
}

/// Hacker — classic green-on-black terminal aesthetic.
/// Multiple shades of green, minimal color palette. Matrix-inspired.
pub fn hacker() -> Theme {
    Theme {
        name: "hacker",
        surface_0: Color::Rgb(4, 8, 4),         // near-black with green tint
        surface_1: Color::Rgb(8, 16, 8),         // very dark green
        surface_1_alt: Color::Rgb(10, 20, 10),   // slightly lighter
        surface_2: Color::Rgb(14, 28, 14),       // dark green
        surface_3: Color::Rgb(22, 44, 22),       // medium dark green
        border_active: Color::Rgb(0, 200, 0),      // bright terminal green
        border_inactive: Color::Rgb(0, 70, 0),     // dim green
        border_accent: Color::Rgb(0, 255, 0),      // full green
        border_subtle: Color::Rgb(0, 35, 0),       // barely visible green
        text_primary: Color::Rgb(0, 220, 0),       // classic green
        text_secondary: Color::Rgb(0, 160, 0),     // medium green
        text_muted: Color::Rgb(0, 90, 0),          // dim green
        text_accent: Color::Rgb(0, 255, 0),        // bright green
        gain_green: Color::Rgb(0, 255, 80),        // bright green with slight cyan
        loss_red: Color::Rgb(180, 0, 0),           // the one red (losses stand out)
        neutral: Color::Rgb(0, 160, 0),
        live_green: Color::Rgb(0, 255, 80),
        stale_yellow: Color::Rgb(0, 180, 0),       // yellow → green to stay on-brand
        key_hint: Color::Rgb(0, 255, 0),
        chart_line: Color::Rgb(0, 200, 40),         // terminal green
        chart_grad_low: Color::Rgb(180, 0, 0),      // red (only non-green color)
        chart_grad_mid: Color::Rgb(0, 150, 0),      // mid green
        chart_grad_high: Color::Rgb(0, 255, 80),    // bright green
        cat_equity: Color::Rgb(0, 200, 60),         // green
        cat_crypto: Color::Rgb(0, 255, 120),        // bright green-cyan
        cat_forex: Color::Rgb(0, 180, 0),           // medium green
        cat_commodity: Color::Rgb(0, 220, 40),      // yellow-green
        cat_fund: Color::Rgb(0, 160, 80),           // teal-green
        cat_cash: Color::Rgb(0, 130, 0),            // muted green
    }
}

/// Pastel — soft, toned-down palette. Muted pinks, blues, greens on warm gray.
/// Easy on the eyes, gentle contrast, like a watercolor wash.
pub fn pastel() -> Theme {
    Theme {
        name: "pastel",
        surface_0: Color::Rgb(42, 40, 45),      // warm dark gray with slight purple
        surface_1: Color::Rgb(50, 48, 54),       // slightly lighter
        surface_1_alt: Color::Rgb(54, 52, 58),   // alt stripe
        surface_2: Color::Rgb(62, 60, 68),       // header/status
        surface_3: Color::Rgb(75, 72, 82),       // selected row
        border_active: Color::Rgb(160, 180, 210),   // soft blue
        border_inactive: Color::Rgb(85, 82, 95),    // muted purple-gray
        border_accent: Color::Rgb(200, 160, 190),   // soft pink
        border_subtle: Color::Rgb(58, 56, 64),      // subtle divider
        text_primary: Color::Rgb(225, 220, 230),    // warm white with lavender tint
        text_secondary: Color::Rgb(170, 165, 180),  // muted lavender
        text_muted: Color::Rgb(110, 105, 120),      // dim purple-gray
        text_accent: Color::Rgb(170, 195, 225),     // soft sky blue
        gain_green: Color::Rgb(140, 200, 160),      // muted sage green
        loss_red: Color::Rgb(210, 130, 140),         // soft rose
        neutral: Color::Rgb(170, 165, 180),
        live_green: Color::Rgb(140, 200, 160),
        stale_yellow: Color::Rgb(220, 195, 140),    // soft gold
        key_hint: Color::Rgb(220, 195, 140),
        chart_line: Color::Rgb(150, 180, 215),       // soft periwinkle
        chart_grad_low: Color::Rgb(210, 130, 140),   // soft rose
        chart_grad_mid: Color::Rgb(220, 195, 140),   // soft gold
        chart_grad_high: Color::Rgb(140, 200, 160),  // soft sage
        cat_equity: Color::Rgb(150, 180, 215),       // periwinkle
        cat_crypto: Color::Rgb(215, 165, 130),       // soft peach
        cat_forex: Color::Rgb(140, 200, 160),        // sage
        cat_commodity: Color::Rgb(220, 195, 140),    // soft gold
        cat_fund: Color::Rgb(185, 155, 200),         // soft lilac
        cat_cash: Color::Rgb(175, 170, 185),         // neutral lavender
    }
}

/// Miasma — warm atmospheric haze. Deep burgundy, dusty orange, olive, muted gold.
/// Like looking through smoke at a sunset. Rich, warm, and slightly ominous.
pub fn miasma() -> Theme {
    Theme {
        name: "miasma",
        surface_0: Color::Rgb(22, 16, 14),      // dark burgundy-brown
        surface_1: Color::Rgb(30, 22, 18),       // warm dark brown
        surface_1_alt: Color::Rgb(35, 25, 20),   // slightly lighter
        surface_2: Color::Rgb(42, 30, 24),       // medium brown
        surface_3: Color::Rgb(58, 40, 30),       // warm brown highlight
        border_active: Color::Rgb(180, 130, 70),    // dusty gold
        border_inactive: Color::Rgb(75, 55, 40),    // dim brown
        border_accent: Color::Rgb(200, 110, 60),    // warm orange
        border_subtle: Color::Rgb(48, 34, 26),      // barely visible
        text_primary: Color::Rgb(220, 200, 175),    // warm parchment
        text_secondary: Color::Rgb(165, 140, 115),  // dusty tan
        text_muted: Color::Rgb(100, 80, 65),        // dark dusty
        text_accent: Color::Rgb(210, 155, 80),      // muted gold
        gain_green: Color::Rgb(145, 165, 75),        // olive green
        loss_red: Color::Rgb(170, 60, 50),           // deep burgundy-red
        neutral: Color::Rgb(165, 140, 115),
        live_green: Color::Rgb(145, 165, 75),
        stale_yellow: Color::Rgb(190, 150, 60),     // aged gold
        key_hint: Color::Rgb(210, 170, 80),
        chart_line: Color::Rgb(200, 140, 65),         // dusty orange
        chart_grad_low: Color::Rgb(150, 45, 40),     // deep burgundy
        chart_grad_mid: Color::Rgb(200, 150, 60),    // dusty gold
        chart_grad_high: Color::Rgb(145, 175, 75),   // olive green
        cat_equity: Color::Rgb(170, 130, 75),         // warm amber
        cat_crypto: Color::Rgb(200, 100, 55),         // rusty orange
        cat_forex: Color::Rgb(145, 165, 75),          // olive
        cat_commodity: Color::Rgb(200, 160, 60),      // aged gold
        cat_fund: Color::Rgb(150, 90, 80),            // dusty rose
        cat_cash: Color::Rgb(160, 145, 125),          // warm stone
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

/// Render a drop shadow on the right and bottom edges of a popup rectangle.
///
/// Height of a section header bar (1 row).
pub const SECTION_HEADER_HEIGHT: u16 = 1;

/// Render a thin section header bar: `"── LABEL ──────────"`.
/// Fills the full width of `area` with a styled line using `text_accent` for the
/// label and `border_subtle` for the decorative rule characters.
/// The background is `surface_2` to give visual separation from the panels below.
pub fn render_section_header(frame: &mut Frame, area: Rect, label: &str, theme: &Theme) {
    if area.height == 0 || area.width < 4 {
        return;
    }

    let prefix = "── ";
    let suffix_char = '─';

    // Build spans: "── LABEL " + fill with ─
    let prefix_span = Span::styled(prefix, Style::default().fg(theme.border_subtle));
    let label_span = Span::styled(
        format!("{} ", label),
        Style::default().fg(theme.text_accent).bold(),
    );

    let used_width = prefix.len() + label.len() + 1; // +1 for trailing space after label
    let remaining = (area.width as usize).saturating_sub(used_width);
    let fill: String = std::iter::repeat_n(suffix_char, remaining).collect();
    let fill_span = Span::styled(fill, Style::default().fg(theme.border_subtle));

    let line = Line::from(vec![prefix_span, label_span, fill_span]);
    let paragraph = ratatui::widgets::Paragraph::new(line)
        .style(Style::default().bg(theme.surface_2));
    frame.render_widget(paragraph, area);
}

/// Draws a 1-cell-wide shadow strip along the right edge and a 1-cell-tall
/// strip along the bottom edge, offset by 1 cell from the popup boundary.
/// The shadow color blends the theme's `surface_0` toward black at
/// [`SHADOW_OPACITY`] intensity, creating a subtle elevated/floating effect.
///
/// Shadow cells that would exceed `area` bounds are silently clipped.
pub fn render_popup_shadow(frame: &mut Frame, popup: Rect, area: Rect, theme: &Theme) {
    let shadow_color = lerp_color(theme.surface_0, Color::Rgb(0, 0, 0), SHADOW_OPACITY);
    let shadow_style = Style::default().bg(shadow_color);

    // Right edge shadow: 1 cell wide, starts 1 row below popup top,
    // height = popup height (so the bottom-right corner overlaps).
    let right_x = popup.x + popup.width;
    let right_y = popup.y + 1;
    let right_h = popup.height;

    if right_x < area.x + area.width {
        let max_y = area.y + area.height;
        for row in right_y..right_y.saturating_add(right_h) {
            if row < max_y {
                let cell = frame.buffer_mut().cell_mut(Position::new(right_x, row));
                if let Some(cell) = cell {
                    cell.set_char(' ');
                    cell.set_style(shadow_style);
                }
            }
        }
    }

    // Bottom edge shadow: 1 cell tall, starts 1 column right of popup left,
    // width = popup width (so the bottom-right corner overlaps).
    let bottom_y = popup.y + popup.height;
    let bottom_x = popup.x + 1;
    let bottom_w = popup.width;

    if bottom_y < area.y + area.height {
        let max_x = area.x + area.width;
        for col in bottom_x..bottom_x.saturating_add(bottom_w) {
            if col < max_x {
                let cell = frame.buffer_mut().cell_mut(Position::new(col, bottom_y));
                if let Some(cell) = cell {
                    cell.set_char(' ');
                    cell.set_style(shadow_style);
                }
            }
        }
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
        assert_eq!(next_theme_name("gruvbox"), "inferno");
        assert_eq!(next_theme_name("miasma"), "midnight"); // wraps around
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

    #[test]
    fn inferno_theme_has_warm_palette() {
        let theme = inferno();
        assert_eq!(theme.name, "inferno");
        // Gains are golden (warm), not green
        match theme.gain_green {
            Color::Rgb(r, _, b) => {
                assert!(r > 200, "inferno gains should be warm (high red)");
                assert!(b < 100, "inferno gains should not be blue");
            }
            _ => panic!("expected Rgb"),
        }
    }

    #[test]
    fn neon_theme_has_vivid_accents() {
        let theme = neon();
        assert_eq!(theme.name, "neon");
        // Border active should be hot pink
        match theme.border_active {
            Color::Rgb(r, _, _) => assert!(r > 200, "neon border should be vivid pink"),
            _ => panic!("expected Rgb"),
        }
        // Chart grad high should be neon green
        match theme.chart_grad_high {
            Color::Rgb(_, g, _) => assert!(g > 200, "neon chart high should be bright green"),
            _ => panic!("expected Rgb"),
        }
    }

    #[test]
    fn hacker_theme_is_mostly_green() {
        let theme = hacker();
        assert_eq!(theme.name, "hacker");
        // Text primary should be green-dominant
        match theme.text_primary {
            Color::Rgb(r, g, b) => {
                assert!(g > 200, "hacker text should be bright green");
                assert!(r < 50, "hacker text should have minimal red");
                assert!(b < 50, "hacker text should have minimal blue");
            }
            _ => panic!("expected Rgb"),
        }
        // Loss red is the only non-green color
        match theme.loss_red {
            Color::Rgb(r, g, _) => {
                assert!(r > 100, "hacker loss should be red");
                assert!(g < 50, "hacker loss should have minimal green");
            }
            _ => panic!("expected Rgb"),
        }
    }

    #[test]
    fn new_themes_load_by_name() {
        assert_eq!(theme_by_name("inferno").name, "inferno");
        assert_eq!(theme_by_name("neon").name, "neon");
        assert_eq!(theme_by_name("hacker").name, "hacker");
    }

    #[test]
    fn theme_count_is_eleven() {
        assert_eq!(THEME_NAMES.len(), 11);
    }

    #[test]
    fn pastel_theme_has_soft_palette() {
        let theme = pastel();
        assert_eq!(theme.name, "pastel");
        // Gains should be muted/soft, not vivid
        match theme.gain_green {
            Color::Rgb(r, g, b) => {
                assert!(g > r, "pastel gains should be green-ish");
                assert!(g < 220, "pastel gains should be soft, not vivid");
                assert!(r > 100, "pastel gains should have warm undertone");
                assert!(b > 100, "pastel gains should be toned (not pure green)");
            }
            _ => panic!("expected Rgb"),
        }
        // Loss red should be soft rose, not harsh
        match theme.loss_red {
            Color::Rgb(r, g, _) => {
                assert!(r > 180, "pastel loss should have red");
                assert!(g > 100, "pastel loss should be soft (not pure red)");
            }
            _ => panic!("expected Rgb"),
        }
    }

    #[test]
    fn miasma_theme_has_warm_brown_palette() {
        let theme = miasma();
        assert_eq!(theme.name, "miasma");
        // Surface should be warm brown (red > blue)
        match theme.surface_0 {
            Color::Rgb(r, _, b) => {
                assert!(r > b, "miasma surface should be warm (red > blue)");
            }
            _ => panic!("expected Rgb"),
        }
        // Gains should be olive green
        match theme.gain_green {
            Color::Rgb(r, g, b) => {
                assert!(g > r, "miasma gains should be green-dominant");
                assert!(b < g, "miasma gains should be olive (low blue)");
            }
            _ => panic!("expected Rgb"),
        }
    }

    #[test]
    fn border_active_has_double_top_single_sides() {
        assert_eq!(BORDER_ACTIVE.horizontal_top, "═");
        assert_eq!(BORDER_ACTIVE.vertical_left, "│");
        assert_eq!(BORDER_ACTIVE.vertical_right, "│");
        assert_eq!(BORDER_ACTIVE.horizontal_bottom, "─");
        assert_eq!(BORDER_ACTIVE.top_left, "╒");
        assert_eq!(BORDER_ACTIVE.top_right, "╕");
    }

    #[test]
    fn border_inactive_is_plain_single_line() {
        assert_eq!(BORDER_INACTIVE.horizontal_top, "─");
        assert_eq!(BORDER_INACTIVE.horizontal_bottom, "─");
        assert_eq!(BORDER_INACTIVE.vertical_left, "│");
        assert_eq!(BORDER_INACTIVE.vertical_right, "│");
        assert_eq!(BORDER_INACTIVE.top_left, "┌");
        assert_eq!(BORDER_INACTIVE.top_right, "┐");
        assert_eq!(BORDER_INACTIVE.bottom_left, "└");
        assert_eq!(BORDER_INACTIVE.bottom_right, "┘");
    }

    #[test]
    fn border_popup_is_full_double_line() {
        use ratatui::symbols::border;
        assert_eq!(BORDER_POPUP, border::DOUBLE);
    }

    #[test]
    fn border_active_and_inactive_differ() {
        // Active and inactive should have visually distinct top borders
        assert_ne!(BORDER_ACTIVE.horizontal_top, BORDER_INACTIVE.horizontal_top);
        assert_ne!(BORDER_ACTIVE.top_left, BORDER_INACTIVE.top_left);
        assert_ne!(BORDER_ACTIVE.top_right, BORDER_INACTIVE.top_right);
    }

    #[test]
    fn shadow_opacity_constant_valid() {
        const { assert!(SHADOW_OPACITY > 0.0, "shadow should be visible") };
        const { assert!(SHADOW_OPACITY <= 1.0, "shadow opacity cannot exceed 1.0") };
    }

    #[test]
    fn shadow_color_is_darker_than_surface() {
        let theme = midnight();
        let shadow = lerp_color(theme.surface_0, Color::Rgb(0, 0, 0), SHADOW_OPACITY);
        // surface_0 for midnight is Rgb(12, 13, 22)
        // Shadow should be darker (closer to black)
        if let (Color::Rgb(sr, sg, sb), Color::Rgb(thr, thg, thb)) = (shadow, theme.surface_0) {
            assert!(sr <= thr, "shadow red should be <= surface red");
            assert!(sg <= thg, "shadow green should be <= surface green");
            assert!(sb <= thb, "shadow blue should be <= surface blue");
        }
    }

    #[test]
    fn shadow_right_edge_placed_correctly() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = midnight();

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 20, 10);
                let popup = Rect::new(5, 2, 8, 4);
                render_popup_shadow(frame, popup, area, &theme);

                // Right shadow should be at x=13 (5+8), rows 3..7 (y+1 to y+height+1)
                let shadow_color =
                    lerp_color(theme.surface_0, Color::Rgb(0, 0, 0), SHADOW_OPACITY);
                for row in 3..7 {
                    let cell = frame.buffer_mut().cell_mut(Position::new(13, row)).unwrap();
                    assert_eq!(
                        cell.bg, shadow_color,
                        "right shadow cell at (13, {row}) should have shadow bg"
                    );
                }
            })
            .unwrap();
    }

    #[test]
    fn shadow_bottom_edge_placed_correctly() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = midnight();

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 20, 10);
                let popup = Rect::new(5, 2, 8, 4);
                render_popup_shadow(frame, popup, area, &theme);

                // Bottom shadow should be at y=6 (2+4), cols 6..14 (x+1 to x+width+1)
                let shadow_color =
                    lerp_color(theme.surface_0, Color::Rgb(0, 0, 0), SHADOW_OPACITY);
                for col in 6..14 {
                    let cell = frame.buffer_mut().cell_mut(Position::new(col, 6)).unwrap();
                    assert_eq!(
                        cell.bg, shadow_color,
                        "bottom shadow cell at ({col}, 6) should have shadow bg"
                    );
                }
            })
            .unwrap();
    }

    #[test]
    fn shadow_clips_to_area_bounds() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(10, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = midnight();

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 10, 8);
                // Popup at bottom-right corner — shadow would go off-screen
                let popup = Rect::new(3, 4, 7, 4);
                // Right edge at x=10 (out of bounds), bottom at y=8 (out of bounds)
                // This should NOT panic
                render_popup_shadow(frame, popup, area, &theme);
            })
            .unwrap();
    }

    #[test]
    fn section_header_height_is_one() {
        assert_eq!(SECTION_HEADER_HEIGHT, 1);
    }

    #[test]
    fn section_header_renders_label() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = midnight();

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 40, 1);
                render_section_header(frame, area, "POSITIONS", &theme);

                // Extract the rendered text from the buffer
                let buf = frame.buffer_mut();
                let mut text = String::new();
                for col in 0..40 {
                    if let Some(cell) = buf.cell(Position::new(col, 0)) {
                        text.push_str(cell.symbol());
                    }
                }
                assert!(text.contains("POSITIONS"), "header should contain label, got: '{}'", text);
                assert!(text.contains("──"), "header should contain rule chars, got: '{}'", text);
            })
            .unwrap();
    }

    #[test]
    fn section_header_uses_surface_2_background() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(30, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = midnight();

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 30, 1);
                render_section_header(frame, area, "TEST", &theme);

                // Check that cells have the surface_2 background
                let buf = frame.buffer_mut();
                if let Some(cell) = buf.cell(Position::new(5, 0)) {
                    assert_eq!(cell.bg, theme.surface_2,
                        "section header should use surface_2 background");
                }
            })
            .unwrap();
    }

    #[test]
    fn section_header_skips_zero_height() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(30, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = midnight();

        terminal
            .draw(|frame| {
                // Zero height area — should not panic
                let area = Rect::new(0, 0, 30, 0);
                render_section_header(frame, area, "TEST", &theme);
            })
            .unwrap();
    }

    #[test]
    fn section_header_skips_narrow_width() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(30, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = midnight();

        terminal
            .draw(|frame| {
                // Width < 4 — should not panic, just skip
                let area = Rect::new(0, 0, 3, 1);
                render_section_header(frame, area, "TEST", &theme);
            })
            .unwrap();
    }

    #[test]
    fn section_header_fills_full_width() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(50, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = midnight();

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 50, 1);
                render_section_header(frame, area, "ASSET OVERVIEW", &theme);

                // The last cell should have surface_2 bg (the fill extends to the edge)
                let buf = frame.buffer_mut();
                if let Some(cell) = buf.cell(Position::new(49, 0)) {
                    assert_eq!(cell.bg, theme.surface_2,
                        "section header fill should reach the right edge");
                }
            })
            .unwrap();
    }

    #[test]
    fn shadow_does_not_touch_popup_top_left() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = midnight();

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 20, 10);
                let popup = Rect::new(5, 2, 8, 4);
                render_popup_shadow(frame, popup, area, &theme);

                let shadow_color =
                    lerp_color(theme.surface_0, Color::Rgb(0, 0, 0), SHADOW_OPACITY);

                // Top-left corner of popup (5, 2) should NOT have shadow
                let cell = frame.buffer_mut().cell_mut(Position::new(5, 2)).unwrap();
                assert_ne!(
                    cell.bg, shadow_color,
                    "popup top-left corner should not have shadow"
                );

                // Cell above right shadow (13, 2) should NOT have shadow
                let cell = frame.buffer_mut().cell_mut(Position::new(13, 2)).unwrap();
                assert_ne!(
                    cell.bg, shadow_color,
                    "cell above right shadow should not have shadow"
                );

                // Cell left of bottom shadow (5, 6) should NOT have shadow
                let cell = frame.buffer_mut().cell_mut(Position::new(5, 6)).unwrap();
                assert_ne!(
                    cell.bg, shadow_color,
                    "cell left of bottom shadow should not have shadow"
                );
            })
            .unwrap();
    }

    fn rgb(color: Color) -> (u8, u8, u8) {
        match color {
            Color::Rgb(r, g, b) => (r, g, b),
            Color::Black => (0, 0, 0),
            Color::White => (255, 255, 255),
            Color::Red => (255, 0, 0),
            Color::Green => (0, 255, 0),
            Color::Blue => (0, 0, 255),
            Color::Yellow => (255, 255, 0),
            Color::Magenta => (255, 0, 255),
            Color::Cyan => (0, 255, 255),
            Color::Gray => (128, 128, 128),
            Color::DarkGray => (64, 64, 64),
            _ => (127, 127, 127),
        }
    }

    fn srgb_to_linear(v: u8) -> f64 {
        let x = v as f64 / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(color: Color) -> f64 {
        let (r, g, b) = rgb(color);
        0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)
    }

    fn contrast_ratio(fg: Color, bg: Color) -> f64 {
        let l1 = luminance(fg);
        let l2 = luminance(bg);
        let (bright, dark) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        (bright + 0.05) / (dark + 0.05)
    }

    #[test]
    fn theme_contrast_guardrails() {
        for name in THEME_NAMES {
            let t = theme_by_name(name);
            let p1 = contrast_ratio(t.text_primary, t.surface_1);
            let p2 = contrast_ratio(t.text_primary, t.surface_0);
            let s1 = contrast_ratio(t.text_secondary, t.surface_1);
            let a1 = contrast_ratio(t.text_accent, t.surface_1);
            assert!(
                p1 >= 4.0,
                "theme={} text_primary/surface_1 contrast too low: {:.2}",
                name,
                p1
            );
            assert!(
                p2 >= 4.5,
                "theme={} text_primary/surface_0 contrast too low: {:.2}",
                name,
                p2
            );
            assert!(
                s1 >= 3.0,
                "theme={} text_secondary/surface_1 contrast too low: {:.2}",
                name,
                s1
            );
            assert!(
                a1 >= 3.0,
                "theme={} text_accent/surface_1 contrast too low: {:.2}",
                name,
                a1
            );
        }
    }
}
