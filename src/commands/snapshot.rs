use anyhow::Result;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

use crate::app::App;
use crate::config::Config;
use crate::db::default_db_path;
use crate::tui::ui;

/// Default snapshot dimensions.
const DEFAULT_WIDTH: u16 = 120;
const DEFAULT_HEIGHT: u16 = 40;

/// Render the TUI to stdout as ANSI-colored text.
pub fn run(config: &Config, width: Option<u16>, height: Option<u16>, plain: bool) -> Result<()> {
    let w = width.unwrap_or(DEFAULT_WIDTH);
    let h = height.unwrap_or(DEFAULT_HEIGHT);

    let db_path = default_db_path();
    let mut app = App::new(config, db_path);
    app.set_terminal_size(w, h);
    app.init_offline();

    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend)?;

    terminal.draw(|frame| {
        ui::render(frame, &mut app);
    })?;

    let buffer = terminal.backend().buffer().clone();

    if plain {
        print_plain(&buffer, w, h);
    } else {
        print_ansi(&buffer, w, h);
    }

    Ok(())
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
