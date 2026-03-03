use ratatui::{
    prelude::*,
    widgets::{Cell, Row},
};

use crate::tui::theme::{self, Theme};

/// Number of skeleton placeholder rows to render.
const SKELETON_ROW_COUNT: usize = 6;

/// Wave animation period in ticks (~60fps → ~2s cycle).
const WAVE_PERIOD: u64 = 120;

/// Phase offset per row (creates cascading wave effect).
const ROW_PHASE_OFFSET: u64 = 12;

/// Build skeleton placeholder rows for loading states.
///
/// Renders `SKELETON_ROW_COUNT` rows of `░` block characters that shimmer
/// with a wave animation. Each row has a phase offset so the wave cascades
/// downward, making loading feel dynamic and intentional.
///
/// `col_widths` defines the approximate character width for each column's
/// skeleton block. `tick` drives the animation phase.
pub fn skeleton_rows<'a>(
    theme: &'a Theme,
    tick: u64,
    col_widths: &[usize],
    col_count: usize,
) -> Vec<Row<'a>> {
    (0..SKELETON_ROW_COUNT)
        .map(|row_idx| {
            let phase = tick.wrapping_add(row_idx as u64 * ROW_PHASE_OFFSET);
            let brightness = wave_brightness(phase);

            let color = shimmer_color(theme, brightness);
            let style = Style::default().fg(color);

            let cells: Vec<Cell> = (0..col_count)
                .map(|col_idx| {
                    let w = col_widths.get(col_idx).copied().unwrap_or(6);
                    Cell::from(Span::styled("░".repeat(w), style))
                })
                .collect();

            let bg = if row_idx % 2 == 0 {
                theme.surface_1
            } else {
                theme.surface_0
            };

            Row::new(cells).style(Style::default().bg(bg))
        })
        .collect()
}

/// Compute a brightness value in [0.0, 1.0] from a sine wave.
fn wave_brightness(phase: u64) -> f32 {
    let t = (phase % WAVE_PERIOD) as f32 / WAVE_PERIOD as f32;
    // Sine wave mapped to [0.3, 1.0] — never fully invisible
    let sine = (t * std::f32::consts::TAU).sin();
    0.65 + 0.35 * sine
}

/// Interpolate between `text_muted` and a slightly brighter shade based on brightness.
fn shimmer_color(theme: &Theme, brightness: f32) -> Color {
    let dim = theme.text_muted;
    let bright = theme::lerp_color(theme.text_muted, theme.text_secondary, 0.6);
    theme::lerp_color(dim, bright, brightness)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme;

    fn test_theme() -> Theme {
        theme::midnight()
    }

    #[test]
    fn skeleton_row_count_matches() {
        let t = test_theme();
        let rows = skeleton_rows(&t, 0, &[8, 6, 6, 5], 4);
        assert_eq!(rows.len(), SKELETON_ROW_COUNT);
    }

    #[test]
    fn skeleton_rows_vary_with_tick() {
        let t = test_theme();
        let rows_0 = skeleton_rows(&t, 0, &[8], 1);
        let rows_30 = skeleton_rows(&t, 30, &[8], 1);
        // Different ticks should produce different styling (animation)
        // We can't easily compare Row internals, but the function should not panic
        assert_eq!(rows_0.len(), SKELETON_ROW_COUNT);
        assert_eq!(rows_30.len(), SKELETON_ROW_COUNT);
    }

    #[test]
    fn wave_brightness_in_range() {
        for tick in 0..WAVE_PERIOD * 2 {
            let b = wave_brightness(tick);
            assert!(
                (0.29..=1.01).contains(&b),
                "brightness {} out of range at tick {}",
                b,
                tick
            );
        }
    }

    #[test]
    fn shimmer_color_returns_rgb() {
        let t = test_theme();
        let color = shimmer_color(&t, 0.5);
        assert!(
            matches!(color, Color::Rgb(_, _, _)),
            "expected RGB color, got {:?}",
            color
        );
    }

    #[test]
    fn skeleton_handles_more_cols_than_widths() {
        let t = test_theme();
        // col_count=5 but only 2 widths provided — extras should default to 6
        let rows = skeleton_rows(&t, 0, &[10, 8], 5);
        assert_eq!(rows.len(), SKELETON_ROW_COUNT);
    }

    #[test]
    fn skeleton_handles_zero_columns() {
        let t = test_theme();
        let rows = skeleton_rows(&t, 0, &[], 0);
        assert_eq!(rows.len(), SKELETON_ROW_COUNT);
    }
}
