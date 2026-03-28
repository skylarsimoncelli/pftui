# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

_(none)_

## P2 - Coverage And Agent Consumption

_(none)_

## P3 - Long Term

_(none)_

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 78% | 75% | Mar 28 | ↑ (72→78 usefulness, 75→75 overall. --claim fix #392 + cross-timeframe #396 + alerts redirect #398 shipped. **Lowest scorer — priority.**) |
| Medium-Timeframe Analyst | 85% | 90% | Mar 28 | ↑ (88→90 overall. Scenario impact-matrix #387 + regime transitions #407 shipped.) |
| Low-Timeframe Analyst | 85% | 90% | Mar 28 | → (stable. Alert triage #405 + regime transitions #407 + cross-timeframe resolve #410 shipped.) |
| High-Timeframe Analyst | 85% | 90% | Mar 26 | → (stable. Scenario suggest #366 shipped.) |
| Morning Intelligence | 75% | 85% | Mar 28 | new (first scored review. Overnight changes #400 shipped.) |
| Morning Brief | 85% | 80% | Mar 28 | → (stable. Morning-brief #363 shipped.) |
| Alert Investigator | 85% | 80-82% | Mar 25-26 | → (stable, consistent.) |
| Public Daily Report | 82% | 80% | Mar 28 | new (first scored review. Commodity coverage #402 shipped.) |
| Dev Agent | 92% | 94% | Mar 28 | → (stable high.) |

**Key changes since last review (Mar 28 prior run):**
- Cross-timeframe disagreement resolution shipped (#410) — addresses low-timeframe-analyst's remaining Mar 28 request.
- All three low-timeframe-analyst Mar 28 requests now shipped: alert triage (#405), regime transition scoring (#407), cross-timeframe resolve (#410).

**Shipped since last TODO update:**
1. ✅ **Cross-timeframe disagreement resolution** — #410. `analytics cross-timeframe --resolve --json`. Weighted priority scoring, stance recommendation, severity classification, resolution triggers.
2. ✅ **Regime transition probability scoring** — #407. `analytics regime-transitions --json`.
3. ✅ **Alert triage dashboard** — #405. `analytics alerts triage --json`.
4. ✅ **Unified cross-timeframe view** — #396. `analytics cross-timeframe --json`.
5. ✅ **Scenario impact matrix** — #387. `analytics scenario impact-matrix --json`.
6. ✅ **Prediction add --claim flag** — #392. Named flag UX fix.
7. ✅ **Overnight price changes in brief** — #400.
8. ✅ **Commodity coverage (URA)** — #402.
9. ✅ **Data alerts redirect** — #398.

**Release eligibility:** 26 commits since v0.19.0 with 9 feature PRs, no P0 bugs, tests (1883) and clippy clean. **Eligible for v0.20.0** — substantial new work shipped.

**GitHub stars:** 7 — Homebrew Core requires 50+.
