//! Ehlers Roofing Filter (ERF) — faithful port of
//! `docs/reference/ehlers-roofing-filter.pine` (everget, MIT).
//!
//! Pipeline: a single-pole high-pass roofing filter removes the low-frequency
//! trend, then a 2-pole (default) or 3-pole super-smoother removes the
//! high-frequency noise. The output oscillates around zero; `erf >= 0` is the
//! positive zone, while the cycle-bottom watch in `cycle_signals` tracks
//! turn-ups from the negative bottom zone.
//!
//! `PI = 2 * asin(1)` exactly as in the Pine. All math is `f64`; no money
//! flows through this module (oscillator only). `nz(x[k])` (Pine's
//! "previous-bar-or-zero") is reproduced by seeding the recurrences with 0.0
//! before warm-up — identical to the Pine on any real history.

/// Pi to full f64 precision, computed the Pine way (`2 * asin(1)`).
fn pi() -> f64 {
    2.0 * 1.0_f64.asin()
}

/// Number of super-smoother poles. Both arms are faithful to the Pine
/// (`numberOfPoles = 2 | 3`); the 3-pole arm + `from_int` are part of the
/// ported public surface (test-exercised) but not yet wired to a CLI flag —
/// the suite uses the 2-pole default. Kept so the port stays complete.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poles {
    Two,
    Three,
}

impl Poles {
    #[allow(dead_code)]
    pub fn from_int(n: u8) -> Option<Self> {
        match n {
            2 => Some(Poles::Two),
            3 => Some(Poles::Three),
            _ => None,
        }
    }
}

/// 2-pole super-smoother (Pine `twoPoleSuperSmootherFilter`).
fn two_pole_super_smoother(src: &[f64], length: usize) -> Vec<f64> {
    let n = src.len();
    let mut ssf = vec![0.0; n];
    if length == 0 {
        return ssf;
    }
    let arg = std::f64::consts::SQRT_2 * pi() / length as f64;
    let a1 = (-arg).exp();
    let b1 = 2.0 * a1 * arg.cos();
    let c2 = b1;
    let c3 = -a1.powi(2);
    let c1 = 1.0 - c2 - c3;
    for i in 0..n {
        let p1 = if i >= 1 { ssf[i - 1] } else { 0.0 };
        let p2 = if i >= 2 { ssf[i - 2] } else { 0.0 };
        ssf[i] = c1 * src[i] + c2 * p1 + c3 * p2;
    }
    ssf
}

/// 3-pole super-smoother (Pine `threePoleSuperSmootherFilter`).
fn three_pole_super_smoother(src: &[f64], length: usize) -> Vec<f64> {
    let n = src.len();
    let mut ssf = vec![0.0; n];
    if length == 0 {
        return ssf;
    }
    let arg = pi() / length as f64;
    let a1 = (-arg).exp();
    let b1 = 2.0 * a1 * (1.738 * arg).cos();
    let c1 = a1.powi(2);
    let coef2 = b1 + c1;
    let coef3 = -(c1 + b1 * c1);
    let coef4 = c1.powi(2);
    let coef1 = 1.0 - coef2 - coef3 - coef4;
    for i in 0..n {
        let p1 = if i >= 1 { ssf[i - 1] } else { 0.0 };
        let p2 = if i >= 2 { ssf[i - 2] } else { 0.0 };
        let p3 = if i >= 3 { ssf[i - 3] } else { 0.0 };
        ssf[i] = coef1 * src[i] + coef2 * p1 + coef3 * p2 + coef4 * p3;
    }
    ssf
}

/// Compute the full Ehlers Roofing Filter series over `src` (closes,
/// oldest-first). Returns `None` when fewer than 4 bars exist (the recurrence
/// needs ≥3 priors to converge). Otherwise returns a series of the same length
/// as `src`.
///
/// Defaults: `highpass_length = 48`, `ssf_length = 10`, `poles = Two`.
pub fn compute_erf(
    src: &[f64],
    highpass_length: usize,
    ssf_length: usize,
    poles: Poles,
) -> Option<Vec<f64>> {
    let n = src.len();
    if n < 4 || highpass_length == 0 || ssf_length == 0 {
        return None;
    }
    // High-pass roofing filter.
    let alpha_arg = 2.0 * pi() / (highpass_length as f64 * std::f64::consts::SQRT_2);
    // Pine guards cos==0 by carrying alpha[1]; with non-degenerate lengths
    // cos(alphaArg) is never 0, so a single scalar alpha is exact.
    let cos_a = alpha_arg.cos();
    let alpha = if cos_a != 0.0 {
        (cos_a + alpha_arg.sin() - 1.0) / cos_a
    } else {
        0.0
    };
    let mut highpass = vec![0.0; n];
    let k1 = (1.0 - alpha / 2.0).powi(2);
    let k2 = 2.0 * (1.0 - alpha);
    let k3 = (1.0 - alpha).powi(2);
    for i in 0..n {
        let s0 = src[i];
        let s1 = if i >= 1 { src[i - 1] } else { 0.0 };
        let s2 = if i >= 2 { src[i - 2] } else { 0.0 };
        let h1 = if i >= 1 { highpass[i - 1] } else { 0.0 };
        let h2 = if i >= 2 { highpass[i - 2] } else { 0.0 };
        highpass[i] = k1 * (s0 - 2.0 * s1 + s2) + k2 * h1 - k3 * h2;
    }
    // Super-smoother input = (highpass + highpass[1]) / 2.
    let mut ss_in = vec![0.0; n];
    for i in 0..n {
        let h1 = if i >= 1 { highpass[i - 1] } else { 0.0 };
        ss_in[i] = (highpass[i] + h1) / 2.0;
    }
    let erf = match poles {
        Poles::Two => two_pole_super_smoother(&ss_in, ssf_length),
        Poles::Three => three_pole_super_smoother(&ss_in, ssf_length),
    };
    Some(erf)
}

/// Compute the ERF with the Pine defaults (48 / 10 / 2-pole).
pub fn compute_erf_default(src: &[f64]) -> Option<Vec<f64>> {
    compute_erf(src, 48, 10, Poles::Two)
}

/// Current ERF value (last bar) of a computed series.
pub fn current(erf: &[f64]) -> Option<f64> {
    erf.last().copied()
}

/// True when the latest ERF value is >= 0 (positive zone).
pub fn is_green(erf: &[f64]) -> Option<bool> {
    erf.last().map(|&v| v >= 0.0)
}

/// True when the ERF ticked up on the latest bar (`erf[0] > erf[1]`).
pub fn turned_up(erf: &[f64]) -> Option<bool> {
    let n = erf.len();
    if n < 2 {
        return None;
    }
    Some(erf[n - 1] > erf[n - 2])
}

/// True when the ERF ticked DOWN on the latest bar (`erf[0] < erf[1]`) — the
/// cycle-TOP mirror of [`turned_up`].
pub fn turned_down(erf: &[f64]) -> Option<bool> {
    let n = erf.len();
    if n < 2 {
        return None;
    }
    Some(erf[n - 1] < erf[n - 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_is_two_asin_one() {
        assert!((pi() - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn too_short_returns_none() {
        assert!(compute_erf_default(&[1.0, 2.0, 3.0]).is_none());
    }

    #[test]
    fn two_pole_recurrence_matches_hand_calc() {
        // Verify the 2-pole super-smoother recurrence on a tiny series by
        // re-deriving the exact coefficients and stepping the recurrence.
        let src = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let length = 10usize;
        let arg = std::f64::consts::SQRT_2 * super::pi() / length as f64;
        let a1 = (-arg).exp();
        let b1 = 2.0 * a1 * arg.cos();
        let c2 = b1;
        let c3 = -a1.powi(2);
        let c1 = 1.0 - c2 - c3;
        let mut expect = vec![0.0; src.len()];
        for i in 0..src.len() {
            let p1 = if i >= 1 { expect[i - 1] } else { 0.0 };
            let p2 = if i >= 2 { expect[i - 2] } else { 0.0 };
            expect[i] = c1 * src[i] + c2 * p1 + c3 * p2;
        }
        let got = two_pole_super_smoother(&src, length);
        for (e, g) in expect.iter().zip(got.iter()) {
            assert!((e - g).abs() < 1e-12, "{e} vs {g}");
        }
    }

    #[test]
    fn erf_sign_flips_at_zero_on_a_cycle() {
        // A clean sine wave (no trend) — the high-pass passes it through and
        // the smoothed output must oscillate, i.e. cross zero (sign flips).
        let n = 400usize;
        let src: Vec<f64> = (0..n)
            .map(|i| 100.0 + 10.0 * (i as f64 / 8.0).sin())
            .collect();
        let erf = compute_erf_default(&src).expect("erf");
        // After warm-up the series must contain both signs.
        let tail = &erf[100..];
        let has_pos = tail.iter().any(|&v| v > 0.0);
        let has_neg = tail.iter().any(|&v| v < 0.0);
        assert!(has_pos && has_neg, "ERF must oscillate around 0");
    }

    #[test]
    fn turned_up_detects_v_bottom() {
        // Down then sharp up. The ERF is a de-trending cycle filter, so it
        // LEADS price: it troughs near the price low and turns up through the
        // reversal. Assert `turned_up` fires on at least one bar in the early
        // recovery window, and that the ERF goes green (>=0) as the rally runs.
        let mut src: Vec<f64> = (0..200).map(|i| 100.0 - i as f64 * 0.3).collect();
        let trough = src.len() - 1;
        let base = *src.last().unwrap();
        for j in 1..=40 {
            src.push(base + j as f64 * 0.6);
        }
        // Scan the bars just after the price trough for the up-turn.
        let mut up_fired = false;
        for end in (trough + 2)..=(trough + 12) {
            let erf = compute_erf_default(&src[..end]).expect("erf");
            if turned_up(&erf) == Some(true) {
                up_fired = true;
                break;
            }
        }
        assert!(up_fired, "ERF should turn up through the reversal");
        // And it should be positive (green) somewhere in the recovery.
        let full = compute_erf_default(&src).expect("erf");
        assert!(
            full[trough..].iter().any(|&v| v >= 0.0),
            "ERF should flip green during the recovery"
        );
    }

    #[test]
    fn turned_down_detects_inverted_v() {
        // Up then sharp down — the ERF leads price, troughing/peaking near the
        // turn. Assert `turned_down` fires on at least one bar in the early
        // decline, and that the ERF goes red (<0) as the selloff runs.
        let mut src: Vec<f64> = (0..200).map(|i| 100.0 + i as f64 * 0.3).collect();
        let peak = src.len() - 1;
        let base = *src.last().unwrap();
        for j in 1..=40 {
            src.push(base - j as f64 * 0.6);
        }
        let mut down_fired = false;
        for end in (peak + 2)..=(peak + 12) {
            let erf = compute_erf_default(&src[..end]).expect("erf");
            if turned_down(&erf) == Some(true) {
                down_fired = true;
                break;
            }
        }
        assert!(down_fired, "ERF should turn down through the reversal");
        let full = compute_erf_default(&src).expect("erf");
        assert!(
            full[peak..].iter().any(|&v| v < 0.0),
            "ERF should flip red during the decline"
        );
    }

    #[test]
    fn three_pole_runs_and_differs_from_two() {
        let src: Vec<f64> = (0..300)
            .map(|i| 100.0 + 5.0 * (i as f64 / 10.0).sin())
            .collect();
        let two = compute_erf(&src, 48, 10, Poles::Two).expect("two");
        let three = compute_erf(&src, 48, 10, Poles::Three).expect("three");
        // Distinct filters → distinct tails.
        assert!((two.last().unwrap() - three.last().unwrap()).abs() > 1e-9);
    }
}
