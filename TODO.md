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
| Evening Analyst | 78% | 75% | Mar 31 | ↓ (88→78 use, 85→75 overall. Backtest revealed 26.7% win rate — worst agent. Over-predicts mean reversion. This is a routine/strategy issue, not a tooling gap.) **Lowest overall scorer — priority.** |
| Medium-Timeframe Analyst | 85% | 88% | Mar 31 | ↑ (85→85 use, 75→88 overall. Major recovery! FRED GDPNow + ISM PMI scraper addressed stale data. PMI discrepancy still noted.) |
| Low-Timeframe Analyst | 85% | 82% | Mar 30 | ↓ (90→85 use, 85→82 overall. FRED API failures. stress-test --list-scenarios shipped.) |
| Macro-Timeframe Analyst | 80% | 85% | Mar 29 | → (stable. Historical regime transitions shipped PR #486.) |
| High-Timeframe Analyst | 85% | 90% | Mar 30 | → (stable.) |
| Morning Intelligence | 75% | 85% | Mar 28 | → (stable.) |
| Morning Brief | 85% | 82% | Mar 30 | → (stable.) |
| Public Daily Report | 82% | 80% | Mar 28 | → (stable.) |
| Dev Agent | 92% | 94% | Mar 31 | → (stable high.) |

**Top 3 priorities based on feedback:**
1. **Evening Analyst prediction quality** — lowest overall at 75%. Backtest shows 26.7% win rate. Not a tooling issue — the analytics pipeline is rated "excellent." The agent routine over-weights mean reversion. Consider adjusting evening-analysis routine to weight momentum signals more heavily.
2. ~~**FRED API resilience**~~ — SHIPPED (#490). Retry + cache fallback + staleness warnings.
3. ~~**PMI data discrepancy**~~ — SHIPPED (#492). Context-aware extraction + broadened regex patterns.

**Shipped since last review (Mar 30):**
1. ✅ FRED GDPNow + Real GDP Growth Rate (#483) — fresher GDP data for Medium-Timeframe Analyst
2. ✅ Regime history date-range filtering + summary (#486) — addresses Macro-Timeframe Analyst request
3. ✅ F57 complete (all 6 sub-items) — timeframe analyst self-awareness
4. ✅ F58 complete (all 4 sub-items) — prediction accuracy backtesting
5. ✅ stress-test --list-scenarios (#463) — Low-Timeframe Analyst request
6. ✅ ISM PMI targeted extraction (#481) — direct ISM data source
7. ✅ FRED API failure resilience (#490) — retry with exponential backoff, cache fallback, staleness warnings
8. ✅ PMI data discrepancy fix (#492) — context-aware extraction, broadened regex patterns, 17 new tests
9. ✅ FIC/MIC conflict monitor (#494) — `analytics power-flow conflicts` with defense vs energy vs VIX cross-reference

**Release status:** v0.23.0 eligible — 37 commits since v0.22.0, no P0 bugs, 2176 tests passing, clippy clean. Features shipped: F57.4-F57.6, F58 complete, regime history filtering, GDPNow, ISM PMI scraper, stress-test --list-scenarios, PMI discrepancy fix, power-flow conflicts.

**GitHub stars:** 8 — Homebrew Core requires 50+.
