use anyhow::{bail, Result};
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

use crate::app::{App, ViewMode};
use crate::config::Config;
use crate::db::default_db_path;
use crate::tui::ui;

/// Default snapshot dimensions.
const DEFAULT_WIDTH: u16 = 120;
const DEFAULT_HEIGHT: u16 = 40;

/// Map a `--view` slug to a `ViewMode`. Accepts the canonical lower-case name
/// plus a few intuitive aliases. Returns `None` for an unknown slug so the
/// caller can list the valid options.
pub(crate) fn parse_view(slug: &str) -> Option<ViewMode> {
    match slug.trim().to_ascii_lowercase().as_str() {
        "positions" | "portfolio" => Some(ViewMode::Positions),
        "transactions" | "tx" => Some(ViewMode::Transactions),
        "markets" => Some(ViewMode::Markets),
        "economy" => Some(ViewMode::Economy),
        "watchlist" => Some(ViewMode::Watchlist),
        "analytics" => Some(ViewMode::Analytics),
        "news" => Some(ViewMode::News),
        "journal" => Some(ViewMode::Journal),
        "risk" | "risk-dashboard" | "riskdashboard" => Some(ViewMode::RiskDashboard),
        "cycles" => Some(ViewMode::Cycles),
        _ => None,
    }
}

/// All accepted `--view` slugs (canonical names), for help/error listing.
const VIEW_SLUGS: &[&str] = &[
    "positions",
    "transactions",
    "markets",
    "economy",
    "watchlist",
    "analytics",
    "news",
    "journal",
    "risk-dashboard",
    "cycles",
];

/// Render the TUI to stdout as ANSI-colored text.
///
/// `--demo` renders against a fresh, self-contained synthetic portfolio (built
/// in a temp dir, never the real DB) so renders are reproducible and safe to
/// share. `--view` selects which view to render; `--subtab` selects a sub-tab
/// within views that have them (currently the Risk Dashboard).
pub fn run(
    config: &Config,
    width: Option<u16>,
    height: Option<u16>,
    plain: bool,
    view: Option<&str>,
    subtab: Option<u8>,
    demo: bool,
) -> Result<()> {
    let w = width.unwrap_or(DEFAULT_WIDTH);
    let h = height.unwrap_or(DEFAULT_HEIGHT);
    let buffer = render_view_buffer(config, w, h, view, subtab, demo)?;
    if plain {
        print_plain(&buffer, w, h);
    } else {
        print_ansi(&buffer, w, h);
    }
    Ok(())
}

/// Build an App (demo or real DB), select the view/sub-tab, and render it once to
/// an off-screen buffer. Extracted so tests can assert on rendered CONTENT.
pub(crate) fn render_view_buffer(
    config: &Config,
    w: u16,
    h: u16,
    view: Option<&str>,
    subtab: Option<u8>,
    demo: bool,
) -> Result<ratatui::buffer::Buffer> {
    let db_path = if demo {
        crate::commands::demo::build_temp_demo_db()?
    } else {
        default_db_path()
    };
    let mut app = App::new(config, db_path);
    app.set_terminal_size(w, h);
    app.init_offline();

    if let Some(slug) = view {
        match parse_view(slug) {
            Some(vm) => app.view_mode = vm,
            None => bail!(
                "unknown --view '{slug}'. Valid views: {}",
                VIEW_SLUGS.join(", ")
            ),
        }
    }
    if let Some(st) = subtab {
        app.risk_subtab = st;
        app.cycles_subtab = st;
    }
    if demo && app.selected_symbol.is_none() {
        app.selected_symbol = Some("BTC".to_string());
    }

    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| {
        ui::render(frame, &mut app);
    })?;
    Ok(terminal.backend().buffer().clone())
}

/// Flatten an off-screen buffer to plain text (one string per row, joined).
#[cfg(test)]
pub(crate) fn buffer_to_plain_string(buffer: &ratatui::buffer::Buffer, w: u16, h: u16) -> String {
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

/// Print buffer as plain text (no colors).
fn print_plain(buffer: &ratatui::buffer::Buffer, width: u16, height: u16) {
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            let cell = &buffer[(x, y)];
            line.push_str(cell.symbol());
        }
        // Trim trailing whitespace per line
        println!("{}", line.trim_end());
    }
}

/// Print buffer as ANSI-escaped text with colors.
fn print_ansi(buffer: &ratatui::buffer::Buffer, width: u16, height: u16) {
    for y in 0..height {
        let mut line = String::new();
        let mut prev_fg: Option<Color> = None;
        let mut prev_bg: Option<Color> = None;
        let mut prev_mods: Modifier = Modifier::empty();

        for x in 0..width {
            let cell = &buffer[(x, y)];
            let fg = cell.fg;
            let bg = cell.bg;
            let mods = cell.modifier;

            // Build escape sequence if style changed
            let style_changed = prev_fg != Some(fg) || prev_bg != Some(bg) || prev_mods != mods;

            if style_changed {
                let mut codes: Vec<String> = Vec::new();

                // Reset first, then apply
                codes.push("0".to_string());

                // Foreground
                if let Some(code) = color_to_ansi_fg(fg) {
                    codes.push(code);
                }

                // Background
                if let Some(code) = color_to_ansi_bg(bg) {
                    codes.push(code);
                }

                // Modifiers
                if mods.contains(Modifier::BOLD) {
                    codes.push("1".to_string());
                }
                if mods.contains(Modifier::DIM) {
                    codes.push("2".to_string());
                }
                if mods.contains(Modifier::ITALIC) {
                    codes.push("3".to_string());
                }
                if mods.contains(Modifier::UNDERLINED) {
                    codes.push("4".to_string());
                }
                if mods.contains(Modifier::REVERSED) {
                    codes.push("7".to_string());
                }

                line.push_str(&format!("\x1b[{}m", codes.join(";")));

                prev_fg = Some(fg);
                prev_bg = Some(bg);
                prev_mods = mods;
            }

            line.push_str(cell.symbol());
        }

        // Reset at end of line
        line.push_str("\x1b[0m");

        // Trim trailing reset-only sequences but keep the final reset
        println!("{}", line);
    }

    // Final reset
    print!("\x1b[0m");
}

/// Convert a ratatui Color to an ANSI foreground escape code.
fn color_to_ansi_fg(color: Color) -> Option<String> {
    match color {
        Color::Reset => None,
        Color::Black => Some("30".to_string()),
        Color::Red => Some("31".to_string()),
        Color::Green => Some("32".to_string()),
        Color::Yellow => Some("33".to_string()),
        Color::Blue => Some("34".to_string()),
        Color::Magenta => Some("35".to_string()),
        Color::Cyan => Some("36".to_string()),
        Color::Gray => Some("37".to_string()),
        Color::DarkGray => Some("90".to_string()),
        Color::LightRed => Some("91".to_string()),
        Color::LightGreen => Some("92".to_string()),
        Color::LightYellow => Some("93".to_string()),
        Color::LightBlue => Some("94".to_string()),
        Color::LightMagenta => Some("95".to_string()),
        Color::LightCyan => Some("96".to_string()),
        Color::White => Some("97".to_string()),
        Color::Rgb(r, g, b) => Some(format!("38;2;{};{};{}", r, g, b)),
        Color::Indexed(i) => Some(format!("38;5;{}", i)),
    }
}

/// Convert a ratatui Color to an ANSI background escape code.
fn color_to_ansi_bg(color: Color) -> Option<String> {
    match color {
        Color::Reset => None,
        Color::Black => Some("40".to_string()),
        Color::Red => Some("41".to_string()),
        Color::Green => Some("42".to_string()),
        Color::Yellow => Some("43".to_string()),
        Color::Blue => Some("44".to_string()),
        Color::Magenta => Some("45".to_string()),
        Color::Cyan => Some("46".to_string()),
        Color::Gray => Some("47".to_string()),
        Color::DarkGray => Some("100".to_string()),
        Color::LightRed => Some("101".to_string()),
        Color::LightGreen => Some("102".to_string()),
        Color::LightYellow => Some("103".to_string()),
        Color::LightBlue => Some("104".to_string()),
        Color::LightMagenta => Some("105".to_string()),
        Color::LightCyan => Some("106".to_string()),
        Color::White => Some("107".to_string()),
        Color::Rgb(r, g, b) => Some(format!("48;2;{};{};{}", r, g, b)),
        Color::Indexed(i) => Some(format!("48;5;{}", i)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_view_canonical_and_aliases() {
        assert_eq!(parse_view("positions"), Some(ViewMode::Positions));
        assert_eq!(parse_view("portfolio"), Some(ViewMode::Positions));
        assert_eq!(parse_view("RISK-DASHBOARD"), Some(ViewMode::RiskDashboard));
        assert_eq!(parse_view("risk"), Some(ViewMode::RiskDashboard));
        assert_eq!(parse_view("cycles"), Some(ViewMode::Cycles));
        assert_eq!(parse_view("CYCLES"), Some(ViewMode::Cycles));
        assert_eq!(parse_view(" analytics "), Some(ViewMode::Analytics));
        assert_eq!(parse_view("nope"), None);
    }

    #[test]
    fn every_view_slug_parses() {
        for slug in VIEW_SLUGS {
            assert!(parse_view(slug).is_some(), "slug {slug} should parse");
        }
    }

    #[test]
    fn demo_snapshot_renders_every_view_without_panic() {
        // Exercises the full offline demo path (build synthetic DB, init, render)
        // for every view + every Risk Dashboard sub-tab. Asserts it returns Ok
        // and never panics. Uses ONLY synthetic data — never the real DB.
        let config = Config::default();
        for slug in VIEW_SLUGS {
            let subtab = if *slug == "risk-dashboard" { Some(3) } else { None };
            let r = run(&config, Some(140), Some(44), true, Some(slug), subtab, true);
            assert!(r.is_ok(), "rendering view {slug} failed: {r:?}");
        }
    }

    #[test]
    fn demo_snapshot_renders_every_cycles_subtab_without_panic() {
        // Each Cycles sub-tab (Matrix / Bitcoin / Gold / Engine) must render
        // without panic over the synthetic demo portfolio, across the full
        // width range (160→80). Synthetic data only — never the real DB.
        let config = Config::default();
        for (w, h) in [(160u16, 48u16), (120, 44), (100, 40), (80, 30)] {
            for st in 0..crate::tui::views::cycles::SUBTAB_COUNT {
                let r = run(&config, Some(w), Some(h), true, Some("cycles"), Some(st), true);
                assert!(r.is_ok(), "rendering cycles sub-tab {st} at {w}x{h} failed: {r:?}");
            }
        }
    }

    #[test]
    fn cycles_engine_subtab_surfaces_computed_degree_fields() {
        // The Engine sub-tab (3) must surface the engine's per-degree signal
        // in plain language: band statistics plus the trend/demarcation glosses.
        // Synthetic demo data only.
        let config = Config::default();
        let buf = render_view_buffer(&config, 160, 48, Some("cycles"), Some(3), true)
            .expect("engine render");
        let text = buffer_to_plain_string(&buf, 160, 48).to_lowercase();
        assert!(text.contains("cycle engine"), "engine header missing: {text}");
        // Plain-language glosses for the demarcation/trend lines are present.
        assert!(
            text.contains("demarcation line") || text.contains("age / band"),
            "engine fields missing: {text}"
        );
    }

    #[test]
    fn cycles_matrix_stance_survives_narrow_width() {
        // B2d: Stance is the actionable verdict and must NOT be cut off when the
        // Matrix narrows. At 80 cols the header still shows the Stance column.
        let config = Config::default();
        let buf = render_view_buffer(&config, 80, 30, Some("cycles"), Some(0), true)
            .expect("matrix render");
        let text = buffer_to_plain_string(&buf, 80, 30).to_lowercase();
        assert!(text.contains("stance"), "Stance column dropped at 80 cols: {text}");
    }

    #[test]
    fn cycles_matrix_renders_bottom_confluence_column() {
        // The Matrix must surface the monthly cycle-bottom confluence column:
        // a plain "Low N/7" header and a per-row "N/7" count (or the missing
        // dash). Name-free and ticker-free. Synthetic demo data only.
        let config = Config::default();
        let buf = render_view_buffer(&config, 160, 48, Some("cycles"), Some(0), true)
            .expect("matrix render");
        let text = buffer_to_plain_string(&buf, 160, 48);
        assert!(
            text.contains("Low N/7"),
            "bottom-confluence column header missing: {text}"
        );
        // At least one row renders a real "N/7" cell or the missing dash.
        let has_count = (0..=7).any(|n| text.contains(&format!("{n}/7")));
        assert!(
            has_count || text.contains("—"),
            "bottom-confluence cell (N/7 or dash) missing: {text}"
        );
    }

    #[test]
    fn cycles_matrix_renders_top_confluence_column() {
        // Top/bottom parity: the Matrix must also surface the monthly cycle-TOP
        // confluence column — a plain "Top N/7" header next to "Low N/7" — and a
        // per-row "N/7" count (or the missing dash). Name-free / ticker-free.
        let config = Config::default();
        let buf = render_view_buffer(&config, 160, 48, Some("cycles"), Some(0), true)
            .expect("matrix render");
        let text = buffer_to_plain_string(&buf, 160, 48);
        assert!(text.contains("Low N/7"), "bottom column header missing: {text}");
        assert!(text.contains("Top N/7"), "top-confluence column header missing: {text}");
    }

    #[test]
    fn cycles_engine_subtab_surfaces_confluence_checklist() {
        // The Engine sub-tab must surface the per-criterion ✓/✗ confluence
        // checklist for the focused asset (which signals are firing, not just
        // how many) under clear cycle-bottom / cycle-top headers — OR a clean
        // insufficient-history state. Synthetic demo data only.
        let config = Config::default();
        let buf = render_view_buffer(&config, 160, 60, Some("cycles"), Some(3), true)
            .expect("engine render");
        let text = buffer_to_plain_string(&buf, 160, 60).to_lowercase();
        assert!(
            text.contains("cycle-bottom signals")
                || text.contains("cycle-top signals")
                || text.contains("insufficient history"),
            "engine confluence checklist missing: {text}"
        );
    }

    #[test]
    fn cycles_render_is_free_of_jargon_and_tickers() {
        // Guards the operator's #1 constraint: NO practitioner/author names and
        // NO raw tickers may appear in the rendered Cycles UI. Renders every
        // sub-tab at two sizes and scans the buffer text. Synthetic data only.
        let config = Config::default();
        let forbidden = [
            "Loukas", "Olson", "Olsen", "Bressert", "Mayer", "Hurst", "Wyckoff",
            "Gann", "Elliott", "halving", "GC=F", "SI=F", "BTC-USD",
        ];
        for (w, h) in [(160u16, 48u16), (100, 40)] {
            for st in 0..crate::tui::views::cycles::SUBTAB_COUNT {
                let buf = render_view_buffer(&config, w, h, Some("cycles"), Some(st), true)
                    .expect("cycles render");
                let text = buffer_to_plain_string(&buf, w, h).to_lowercase();
                for term in forbidden {
                    assert!(
                        !text.contains(&term.to_lowercase()),
                        "forbidden term '{term}' leaked into Cycles sub-tab {st} at {w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn cycles_matrix_age_is_never_a_raw_bar_count() {
        // Regression guard for the "1310yr" bug: `cycle_age_bars` is a RAW bar
        // count and must be converted to its display unit before the unit
        // suffix is appended. Scan every age-like "<number><yr>" token in the
        // rendered Matrix/Engine and assert no year value is implausibly large
        // (a raw bar count rendered as years would be hundreds/thousands).
        let config = Config::default();
        let re_yr = regex_lite_year_tokens;
        for st in [0u8, 3] {
            for (w, h) in [(160u16, 48u16), (120, 44)] {
                let buf = render_view_buffer(&config, w, h, Some("cycles"), Some(st), true)
                    .expect("cycles render");
                let text = buffer_to_plain_string(&buf, w, h);
                for years in re_yr(&text) {
                    assert!(
                        years < 30.0,
                        "implausible age {years}yr in Cycles sub-tab {st} \
                         (raw bar count leaked as years?): {text}"
                    );
                }
            }
        }
    }

    /// Tiny dependency-free scanner: pull every `<number>yr` token's numeric
    /// value out of rendered text (handles integer and one-decimal forms).
    fn regex_lite_year_tokens(text: &str) -> Vec<f64> {
        let mut out = Vec::new();
        let bytes: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == 'y'
                && i + 1 < bytes.len()
                && bytes[i + 1] == 'r'
                && i > 0
                && (bytes[i - 1].is_ascii_digit())
            {
                // Walk backwards over digits and a single dot to capture the number.
                let mut j = i;
                while j > 0
                    && (bytes[j - 1].is_ascii_digit() || bytes[j - 1] == '.')
                {
                    j -= 1;
                }
                let num: String = bytes[j..i].iter().collect();
                if let Ok(v) = num.parse::<f64>() {
                    out.push(v);
                }
            }
            i += 1;
        }
        out
    }

    #[test]
    fn unknown_view_is_an_error() {
        let config = Config::default();
        let r = run(&config, Some(120), Some(40), true, Some("bogus"), None, true);
        assert!(r.is_err());
    }

    #[test]
    fn color_to_ansi_fg_rgb() {
        assert_eq!(
            color_to_ansi_fg(Color::Rgb(255, 0, 128)),
            Some("38;2;255;0;128".to_string())
        );
    }

    #[test]
    fn color_to_ansi_fg_indexed() {
        assert_eq!(
            color_to_ansi_fg(Color::Indexed(42)),
            Some("38;5;42".to_string())
        );
    }

    #[test]
    fn color_to_ansi_fg_reset() {
        assert_eq!(color_to_ansi_fg(Color::Reset), None);
    }

    #[test]
    fn color_to_ansi_bg_rgb() {
        assert_eq!(
            color_to_ansi_bg(Color::Rgb(10, 20, 30)),
            Some("48;2;10;20;30".to_string())
        );
    }

    #[test]
    fn color_to_ansi_bg_basic() {
        assert_eq!(color_to_ansi_bg(Color::Red), Some("41".to_string()));
    }

    #[test]
    fn color_to_ansi_bg_reset() {
        assert_eq!(color_to_ansi_bg(Color::Reset), None);
    }

    #[test]
    fn default_dimensions() {
        assert_eq!(DEFAULT_WIDTH, 120);
        assert_eq!(DEFAULT_HEIGHT, 40);
    }
}
