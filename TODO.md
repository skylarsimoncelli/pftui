# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

_(none)_

## P2 - Coverage And Agent Consumption

- [ ] [Feedback] Overnight price change summary in portfolio brief — Morning Intelligence (75/85 Mar 28) wants overnight price moves surfaced directly in `portfolio brief --json`. Add `overnight_changes` section with previous close → current for key holdings. See `src/commands/portfolio_brief.rs`.
- [ ] [Feedback] Commodity coverage in scoreboard — Public Daily Report (82/80 Mar 28) notes uranium and copper missing from price scoreboard tables. Ensure `data prices` and related commands include commodity ETFs/symbols (U-U.TO, COPX, HG=F) when available. Check `src/commands/prices.rs` symbol lists.

## P3 - Long Term

_(none)_

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 78% | 75% | Mar 28 | ↑ (72→78 usefulness, 75→75 overall. --claim fix #392 shipped. Wants unified cross-timeframe view. **Lowest scorer — priority.**) |
| Medium-Timeframe Analyst | 85% | 90% | Mar 28 | ↑ (88→90 overall. Scenario impact-matrix #387 shipped.) |
| Low-Timeframe Analyst | 85% | 90% | Mar 27 | → (stable. Synthesis power structure #384 shipped.) |
| High-Timeframe Analyst | 85% | 90% | Mar 26 | → (stable. Scenario suggest #366 shipped.) |
| Morning Intelligence | 75% | 85% | Mar 28 | new (first scored review. Wants overnight price changes in brief.) |
| Morning Brief | 85% | 88% | Mar 26 | → (stable. Morning-brief #363 shipped.) |
| Alert Investigator | 85% | 80-82% | Mar 25-26 | → (stable, consistent.) |
| Public Daily Report | 82% | 80% | Mar 28 | new (first scored review. Wants better commodity coverage.) |
| Dev Agent | 92% | 94% | Mar 28 | → (stable high.) |

**Key changes since last review (Mar 27):**
- Evening Analyst usefulness up 72→78 (--claim fix helped), overall stable at 75. Still lowest.
- Medium-Timeframe Analyst overall up 88→90 — scenario impact-matrix praised.
- Two new testers appeared: Morning Intelligence (75/85) and Public Daily Report (82/80).
- 21 commits since v0.19.0 including 3 feature PRs (#384, #387, #392), data source fixes, systemd services.

**Shipped since last review (Mar 27):**
1. ✅ **Unified cross-timeframe view** — #396. `analytics cross-timeframe --json`. Alignment + divergence + correlation breaks in one call.
2. ✅ **Scenario impact matrix** — #387. `analytics scenario impact-matrix --json`.
3. ✅ **Prediction add --claim flag** — #392. Named flag UX fix for evening analyst.
4. ✅ **Synthesis power structure integration** — #384. FIC/MIC/TIC in synthesis.
5. ✅ **Economy indicator confidence depth** — #381. 15 FRED indicators, confidence reasoning.
6. ✅ **Data source resilience** — #380. BLS rate limits, broken pipelines fixed.
7. ✅ **Systemd services** — daemon + mobile service files deployed.

**Top 2 priorities from feedback:**
1. **P2: Overnight price changes in brief** — Morning Intelligence wants overnight moves in portfolio brief.
2. **P2: Commodity coverage in scoreboard** — Public Daily Report wants uranium/copper in price tables.

**Release eligibility:** 21 commits since v0.19.0 with 3 feature PRs, no P0 bugs, tests (1822) and clippy clean. **Eligible for v0.20.0** — meaningful new work shipped.

**GitHub stars:** 7 — Homebrew Core requires 50+.
