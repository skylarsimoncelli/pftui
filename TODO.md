# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

- [Feedback] **FRED API failure resilience** — Low-Timeframe Analyst (85/82 Mar 30) reported FRED API failures disrupting macro data flow. Add retry logic with exponential backoff, cache-hit fallback when FRED returns errors, and surface staleness warnings in `data economy --json` output so agents know when they're working with degraded data. Files: `src/data/fred.rs`, `src/commands/economy.rs`.

## P2 - Coverage And Agent Consumption

- [Feedback] **FIC/MIC power balance indicators for conflicts** — Medium-Timeframe Analyst (85/88 Mar 31) wants defense stocks vs oil tracking during geopolitical conflicts. Add defense sector ETFs (ITA, XAR, PPA) to `analytics regime-flows` or a new `analytics power-flow conflicts` subcommand that cross-references energy (XLE, CL=F) with defense (ITA) and VIX during crisis regimes. Files: `src/commands/regime_flows.rs`, `src/data/market_symbols.rs`.
- [Feedback] **PMI data discrepancy investigation** — Medium-Timeframe Analyst (85/88 Mar 31) noted PMI showing 30 vs forecast 51.2 — a 21-point gap suggesting data quality issue in ISM scraper or FRED mapping. Investigate whether ISM scraper (`src/data/ism.rs`) is pulling stale or misformatted values. Validate PMI plausibility range and add cross-source sanity checks.

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
2. **FRED API resilience** — Low-Timeframe Analyst hit FRED failures disrupting macro data. Add retry/fallback/staleness warnings.
3. **PMI data discrepancy** — Medium-Timeframe Analyst noted PMI 30 vs forecast 51.2. Investigate ISM scraper accuracy.

**Shipped since last review (Mar 30):**
1. ✅ FRED GDPNow + Real GDP Growth Rate (#483) — fresher GDP data for Medium-Timeframe Analyst
2. ✅ Regime history date-range filtering + summary (#486) — addresses Macro-Timeframe Analyst request
3. ✅ F57 complete (all 6 sub-items) — timeframe analyst self-awareness
4. ✅ F58 complete (all 4 sub-items) — prediction accuracy backtesting
5. ✅ stress-test --list-scenarios (#463) — Low-Timeframe Analyst request
6. ✅ ISM PMI targeted extraction (#481) — direct ISM data source

**Release status:** v0.23.0 eligible — 35 commits since v0.22.0, no P0 bugs, 2142 tests passing, clippy clean. Features shipped: F57.4-F57.6, F58 complete, regime history filtering, GDPNow, ISM PMI scraper, stress-test --list-scenarios.

**GitHub stars:** 8 — Homebrew Core requires 50+.
