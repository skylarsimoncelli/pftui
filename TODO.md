# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

_(none)_

## P2 - Coverage And Agent Consumption

### [Feedback] Automatic event detection for scenario creation
**Source:** Evening Analysis (Apr 3, 82/78).
**Why:** When major macro events occur (e.g. Warsh nomination, tariff announcements, FOMC surprises), agents should automatically create or suggest new scenarios. Currently agents must manually identify events and run `journal scenario add`. Auto-detection from news/calendar/catalyst feeds would close this gap.
**Scope:** Detect high-impact events from news sentiment spikes + catalyst scoring, auto-suggest `journal scenario add` with pre-filled parameters. Could integrate into `analytics guidance` or as a standalone `analytics scenario detect`.
**Effort:** 1-2 weeks.



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
| Evening Analyst | 72% | 68% | Apr 1 | ↓ (78→72 use, 75→68 overall. Backtest 26.7% WR — routine/strategy issue, not tooling.) **Lowest overall scorer.** |
| Evening Analysis | 82% | 78% | Apr 4 | → (82→82 use, 78→78 overall. Holiday-aware staleness shipped #606. Wants auto event detection + Yahoo rate-limit fix.) |
| Medium-Timeframe Analyst | 85% | 88% | Apr 3 | ↑ (75→85 use, 80→88 overall. Alert thresholds shipped #572.) |
| Low-Timeframe Analyst | 85% | 88% | Apr 3 | ↑ (85→85 use, 80→88 overall. Break history shipped #588.) |
| Macro-Timeframe Analyst | 80% | 85% | Mar 29 | → (stable.) |
| High-Timeframe Analyst | 85% | 90% | Mar 30 | → (stable.) |
| Morning Intelligence | 75% | 85% | Mar 28 | → (stable.) |
| Morning Brief Cron | 85% | 80% | Apr 2 | → (stable.) |
| Morning Brief | 85% | 88% | Apr 3 | ↑ (85→85 use, 80→88 overall.) |
| Public Daily Report | 82% | 80% | Mar 28 | → (stable.) |
| Dev Agent | 92% | 94% | Apr 3 | → (stable high.) |

**Top 3 priorities based on feedback:**
1. **Evening Analyst prediction quality** — lowest overall at 68%. Backtest shows 26.7% win rate. Not a tooling issue — routine over-weights mean reversion. Backtest diagnostics (#525) surfaces this automatically.
2. **Evening Analysis auto-event detection** — 82/78. Wants automatic scenario creation when major events occur. P2 item above.
3. **Yahoo Finance rate-limit resilience** — ✅ Complete. Semaphore concurrency (#615), retry (#609), partial-success (#613), macro indicators (#611) all shipped.

**Shipped since last review (Apr 3):**
1. ✅ Configurable alert thresholds for correlation breaks + scenario probability shifts (#572)
2. ✅ Portfolio snapshot alias for portfolio status (#575)
3. ✅ Correlation break historical context + confirmation tracking (#588)
4. ✅ N+1 query fixes: trends (#579), situation room (#581), movers (#590), snapshots (#593)
5. ✅ --timing global flag for CLI latency monitoring (#583)
6. ✅ Flaky World Bank tests marked #[ignore] (#585)
7. ✅ --newly-triggered/--kind/--condition/--symbol/--status filters on alerts check (#596, #601)
8. ✅ Stale/missing analyst views in analytics guidance (#598)
9. ✅ Holiday-aware staleness on data prices + market-snapshot (#606)
10. ✅ Polymarket pipeline fix + 6 new tag slugs (#607)
11. ✅ Postgres timestamp parsing fixes (#603, #604)
12. ✅ Research-ingestion skill + routine integration
13. ✅ Yahoo Finance retry with exponential backoff (#609)
14. ✅ Macro market indicators in asset_names registry (#611)
15. ✅ Partial-success reporting for price refresh pipeline (#613)
16. ✅ Yahoo Finance semaphore-based concurrency limiting (#615)

**Release status:** v0.26.0 released Apr 4. 2500 tests passing, clippy clean. No P0 bugs.

**GitHub stars:** 9 — Homebrew Core requires 50+.
