//! Bounded-projection target solver — the load-bearing normalizer for the
//! positioning modeller (POSITIONING-MODELS.md §3.2 Phase D).
//!
//! Given a desired class/bucket weight vector and per-bucket `[floor, ceiling]`
//! boxes (cash is just another bucket), produce the weight vector that lies on
//! the probability simplex (`Σ = 1`), inside every box, and is **closest** (in
//! squared Euclidean distance) to the desired vector.
//!
//! This is *not* fixpoint iteration. It is the exact Euclidean projection onto
//! `{x : floorᵢ ≤ xᵢ ≤ ceilingᵢ, Σx = 1}`. The KKT solution has the closed form
//!
//! ```text
//!     xᵢ(λ) = clamp(desiredᵢ − λ, floorᵢ, ceilingᵢ)
//! ```
//!
//! where the single scalar `λ` is chosen so that `Σ xᵢ(λ) = 1`. `g(λ) = Σ xᵢ(λ)`
//! is continuous, piecewise-linear and monotonically non-increasing in `λ`, so
//! the root is unique. We locate the linear segment that brackets `g = 1` using
//! the sorted break-points `desiredᵢ − floorᵢ` and `desiredᵢ − ceilingᵢ`, then
//! solve `λ` exactly inside that segment (a linear equation in `Decimal`). Fully
//! deterministic: the break-point list is sorted with a total `Decimal` order,
//! ties broken by stable bucket index, and the final rounding residual is
//! assigned to a single deterministically-chosen bucket.

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

/// One bucket fed to the solver. `desired` is the (post action-algebra) target;
/// `floor`/`ceiling` are the hard box constraints. Cash is passed as an ordinary
/// bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolveBucket {
    pub key: String,
    pub desired: Decimal,
    pub floor: Decimal,
    pub ceiling: Decimal,
}

impl SolveBucket {
    pub fn new(
        key: impl Into<String>,
        desired: Decimal,
        floor: Decimal,
        ceiling: Decimal,
    ) -> Self {
        Self {
            key: key.into(),
            desired,
            floor,
            ceiling,
        }
    }
}

/// Outcome of a solve. `Solved` weights are index-aligned with the input
/// buckets, rounded to [`WEIGHT_DP`] places, and sum to exactly `1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SolveOutcome {
    Solved(Vec<Decimal>),
    /// `Σ floor > 1` or `Σ ceiling < 1` — no point on the simplex satisfies the
    /// boxes. Caller HOLDS its prior weights.
    Infeasible,
}

/// Fixed decimal precision for solved weights.
pub const WEIGHT_DP: u32 = 8;

/// Project `desired` onto the box-constrained simplex. See module docs.
pub fn solve_targets(buckets: &[SolveBucket]) -> Result<SolveOutcome> {
    if buckets.is_empty() {
        bail!("solve_targets: empty bucket set");
    }
    let one = dec!(1);

    // Sanity: a well-formed box has floor ≤ ceiling and both in [0, 1].
    for b in buckets {
        if b.floor > b.ceiling {
            bail!(
                "solve_targets: bucket {} has floor {} > ceiling {}",
                b.key,
                b.floor,
                b.ceiling
            );
        }
        if b.floor < dec!(0) || b.ceiling > one {
            bail!(
                "solve_targets: bucket {} box [{}, {}] outside [0, 1]",
                b.key,
                b.floor,
                b.ceiling
            );
        }
    }

    // Feasibility: the simplex must intersect the box.
    let sum_floor: Decimal = buckets.iter().map(|b| b.floor).sum();
    let sum_ceil: Decimal = buckets.iter().map(|b| b.ceiling).sum();
    if sum_floor > one || sum_ceil < one {
        return Ok(SolveOutcome::Infeasible);
    }

    // g(λ) = Σ clamp(desiredᵢ − λ, floorᵢ, ceilingᵢ).
    let g = |lam: Decimal| -> Decimal {
        buckets
            .iter()
            .map(|b| clamp(b.desired - lam, b.floor, b.ceiling))
            .sum()
    };

    // Candidate break-points: λ at which a bucket switches clamp state.
    let mut breaks: Vec<Decimal> = Vec::with_capacity(buckets.len() * 2);
    for b in buckets {
        breaks.push(b.desired - b.floor); // hits floor for λ ≥ this
        breaks.push(b.desired - b.ceiling); // hits ceiling for λ ≤ this
    }
    breaks.sort();
    breaks.dedup();

    // Sentinels just outside the break-point range: below the min every bucket
    // is at its ceiling (g = Σceiling ≥ 1), above the max every bucket is at its
    // floor (g = Σfloor ≤ 1). The root therefore lies inside [lo, hi].
    let lo = breaks.first().copied().unwrap_or(dec!(0)) - one;
    let hi = breaks.last().copied().unwrap_or(dec!(0)) + one;
    let mut bounds: Vec<Decimal> = Vec::with_capacity(breaks.len() + 2);
    bounds.push(lo);
    bounds.extend(breaks.iter().copied());
    bounds.push(hi);

    // Walk segments oldest→newest; g is non-increasing so the first segment that
    // brackets g = 1 owns the root. Deterministic by ascending λ.
    let mut lambda = hi;
    for w in bounds.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ga = g(a);
        let gb = g(b);
        if ga >= one && one >= gb {
            // Membership is fixed strictly inside (a, b); sample the midpoint
            // (which never lands on a break-point) to classify buckets.
            let mid = (a + b) / dec!(2);
            let mut free_desired_sum = dec!(0);
            let mut clamped_sum = dec!(0);
            let mut n_free: u32 = 0;
            for bk in buckets {
                let v = bk.desired - mid;
                if v <= bk.floor {
                    clamped_sum += bk.floor;
                } else if v >= bk.ceiling {
                    clamped_sum += bk.ceiling;
                } else {
                    free_desired_sum += bk.desired;
                    n_free += 1;
                }
            }
            lambda = if n_free == 0 {
                // g is constant on this segment and equals 1 (it brackets the
                // root): every bucket is clamped, λ = a reproduces it.
                a
            } else {
                // Σ_free (desiredᵢ − λ) + clamped_sum = 1  ⇒  solve λ.
                (clamped_sum + free_desired_sum - one) / Decimal::from(n_free)
            };
            break;
        }
    }

    // Realize weights at the solved λ, then round + repair the residual so the
    // vector sums to EXACTLY 1.
    let raw: Vec<Decimal> = buckets
        .iter()
        .map(|b| clamp(b.desired - lambda, b.floor, b.ceiling))
        .collect();
    let weights = round_and_repair(buckets, &raw, lambda);
    Ok(SolveOutcome::Solved(weights))
}

#[inline]
fn clamp(v: Decimal, lo: Decimal, hi: Decimal) -> Decimal {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Round each weight to [`WEIGHT_DP`] and force the sum to exactly 1 by assigning
/// the residual to one deterministically-chosen bucket: the *unclamped* (free at
/// λ) bucket with the largest rounded weight, ties broken by lowest index. If no
/// bucket is free, the largest weight overall absorbs it.
fn round_and_repair(buckets: &[SolveBucket], raw: &[Decimal], lambda: Decimal) -> Vec<Decimal> {
    let one = dec!(1);
    let mut rounded: Vec<Decimal> = raw.iter().map(|w| w.round_dp(WEIGHT_DP)).collect();
    let sum: Decimal = rounded.iter().sum();
    let residual = one - sum;
    if residual == dec!(0) {
        return rounded;
    }

    // Free buckets: strictly inside the box at the solved λ.
    let mut target: Option<usize> = None;
    let mut best = Decimal::MIN;
    for (i, b) in buckets.iter().enumerate() {
        let v = b.desired - lambda;
        let free = v > b.floor && v < b.ceiling;
        if free && rounded[i] > best {
            best = rounded[i];
            target = Some(i);
        }
    }
    let idx = target.unwrap_or_else(|| {
        // No free bucket: absorb into the largest weight, ties → lowest index.
        let mut bi = 0usize;
        let mut bw = rounded[0];
        for (i, w) in rounded.iter().enumerate().skip(1) {
            if *w > bw {
                bw = *w;
                bi = i;
            }
        }
        bi
    });
    rounded[idx] += residual;
    rounded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feasible_identity_returns_input() {
        // Desired already sums to 1 and sits inside every box → identity.
        let buckets = vec![
            SolveBucket::new("cash", dec!(0.2), dec!(0), dec!(1)),
            SolveBucket::new("alpha", dec!(0.4), dec!(0), dec!(1)),
            SolveBucket::new("beta", dec!(0.4), dec!(0), dec!(1)),
        ];
        let out = solve_targets(&buckets).unwrap();
        assert_eq!(
            out,
            SolveOutcome::Solved(vec![dec!(0.20000000), dec!(0.40000000), dec!(0.40000000)])
        );
    }

    #[test]
    fn ceiling_violation_redistributes() {
        // desired [.5, .3, .2], ceiling on bucket0 = .4. Hand-derived solution:
        // bucket0 pinned to .4, λ = -0.05, b1 = .35, b2 = .25.
        let buckets = vec![
            SolveBucket::new("a", dec!(0.5), dec!(0), dec!(0.4)),
            SolveBucket::new("b", dec!(0.3), dec!(0), dec!(1)),
            SolveBucket::new("c", dec!(0.2), dec!(0), dec!(1)),
        ];
        let out = solve_targets(&buckets).unwrap();
        assert_eq!(
            out,
            SolveOutcome::Solved(vec![dec!(0.40000000), dec!(0.35000000), dec!(0.25000000)])
        );
    }

    #[test]
    fn infeasible_when_floors_exceed_one() {
        let buckets = vec![
            SolveBucket::new("a", dec!(0.4), dec!(0.5), dec!(1)),
            SolveBucket::new("b", dec!(0.3), dec!(0.4), dec!(1)),
            SolveBucket::new("c", dec!(0.3), dec!(0.3), dec!(1)),
        ];
        let out = solve_targets(&buckets).unwrap();
        assert_eq!(out, SolveOutcome::Infeasible);
    }

    #[test]
    fn infeasible_when_ceilings_below_one() {
        let buckets = vec![
            SolveBucket::new("a", dec!(0.4), dec!(0), dec!(0.3)),
            SolveBucket::new("b", dec!(0.3), dec!(0), dec!(0.3)),
            SolveBucket::new("c", dec!(0.3), dec!(0), dec!(0.3)),
        ];
        assert_eq!(solve_targets(&buckets).unwrap(), SolveOutcome::Infeasible);
    }

    #[test]
    fn solved_weights_always_sum_to_one() {
        // A floor that forces redistribution + rounding residual repair.
        let buckets = vec![
            SolveBucket::new("a", dec!(0.1), dec!(0.3), dec!(1)),
            SolveBucket::new("b", dec!(0.45), dec!(0), dec!(1)),
            SolveBucket::new("c", dec!(0.45), dec!(0), dec!(1)),
        ];
        let out = solve_targets(&buckets).unwrap();
        if let SolveOutcome::Solved(w) = out {
            let sum: Decimal = w.iter().sum();
            assert_eq!(sum, dec!(1));
            // bucket a pinned up to its floor .3; remaining .7 split equally.
            assert_eq!(w, vec![dec!(0.30000000), dec!(0.35000000), dec!(0.35000000)]);
        } else {
            panic!("expected Solved");
        }
    }
}
