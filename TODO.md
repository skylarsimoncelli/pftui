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

### F59: Capital Flow Tracking
**Source:** Competitive research (NOFX institutional flow data).
**Why:** Institutional fund flows, ETF creation/redemption, and open interest changes reveal positioning that price alone doesn't show. pftui has BTC ETF flows and COT but lacks broader equity/commodity fund flow data.
**Scope:** New `data flows` source pulling ETF flow data (ETF.com or similar), institutional 13F filings, and crypto exchange flow data. New table `capital_flows`. Integration into agent routines.
**Effort:** 3-4 weeks. **Priority:** P3 — enhances analysis but agents can web_search for this.



---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 72% | 68% | Apr 1 | ↓ (78→72 use, 75→68 overall. Backtest shows 26.7% win rate — this is a routine/strategy issue, not tooling. `portfolio status` shipped #514.) **Lowest overall scorer — priority.** |
| Medium-Timeframe Analyst | 75% | 80% | Apr 2 | ↓ (85→75 use, 90→80 overall. Synthesis conviction matrix shipped #540.) |
| Evening Analysis | 82% | 80% | Apr 2 | → (new tester entry. `data prices` empty — P1 investigation needed. Wants unified market snapshot.) |
| Low-Timeframe Analyst | 85% | 82% | Apr 1 | → (stable. Correlation break severity #531, alert impact #533, regime confidence-trend #536 all shipped.) |
| Macro-Timeframe Analyst | 80% | 85% | Mar 29 | → (stable. Historical regime transitions shipped #486.) |
| High-Timeframe Analyst | 85% | 90% | Mar 30 | → (stable. Trend evidence enrichment shipped #502.) |
| Morning Intelligence | 75% | 85% | Mar 28 | → (stable.) |
| Morning Brief | 85% | 80% | Apr 1 | → (stable. Watchlist levels in brief shipped #528.) |
| Public Daily Report | 82% | 80% | Mar 28 | → (stable.) |
| Dev Agent | 92% | 94% | Apr 2 | → (stable high.) |

**Top 3 priorities based on feedback:**
1. **Evening Analyst prediction quality** — lowest overall at 68%. Backtest shows 26.7% win rate. Not a tooling issue — the analytics pipeline is rated "excellent." The agent routine over-weights mean reversion. Backtest diagnostics (#525) now surfaces this automatically.
2. **`data prices`/`data quotes` empty output** — P1 investigation. Evening Analysis (Apr 2) reports empty output. May be cache/refresh timing issue.
3. **Medium-Timeframe usability drop** — 85→75 usefulness. Conviction matrix shipped (#540) addresses the main request. Monitor next review.

**Shipped since last review (Apr 1):**
1. ✅ Synthesis conviction matrix (#540) — per-asset analyst conviction scores inline in synthesis
2. ✅ Time-bomb test fix (#544) — dynamic dates in power_flows tests
3. ✅ Unified market snapshot (#548) — prices + sentiment + regime in one call (addresses Evening Analysis P2)

**Release status:** v0.24.0 tagged Apr 2. 71 commits since v0.23.0, no P0 bugs, 2315 tests passing, clippy clean.

**GitHub stars:** 9 — Homebrew Core requires 50+.
