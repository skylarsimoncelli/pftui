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
| Medium-Timeframe Analyst | 85% | 88% | Apr 3 | ↑ (75→85 use, 80→88 overall. Alert thresholds shipped #572. Recovery.) |
| Evening Analysis | 82% | 78% | Apr 3 | ↓ (82→82 use, 80→78 overall. Portfolio snapshot alias shipped #575. Wants auto event detection for scenarios.) |
| Low-Timeframe Analyst | 85% | 88% | Apr 3 | ↑ (85→85 use, 80→88 overall. Break history shipped #588.) |
| Macro-Timeframe Analyst | 80% | 85% | Mar 29 | → (stable.) |
| High-Timeframe Analyst | 85% | 90% | Mar 30 | → (stable.) |
| Morning Intelligence | 75% | 85% | Mar 28 | → (stable.) |
| Morning Brief Cron | 85% | 80% | Apr 2 | → (stable.) |
| Morning Brief | 85% | 88% | Apr 3 | ↑ (85→85 use, 80→88 overall. ISM services already in brief; no code change needed.) |
| Public Daily Report | 82% | 80% | Mar 28 | → (stable.) |
| Dev Agent | 92% | 94% | Apr 3 | → (stable high.) |

**Top 3 priorities based on feedback:**
1. **Evening Analyst prediction quality** — lowest overall at 68%. Backtest shows 26.7% win rate. Not a tooling issue — routine over-weights mean reversion. Backtest diagnostics (#525) surfaces this automatically.
2. **Evening Analysis auto-event detection** — 82/78. Portfolio-matrix coverage at 4%. Wants automatic scenario creation when major events occur. New P2 item added.
3. **Morning Intelligence stale** — 75/85 since Mar 28. No new feedback. Monitor.

**Shipped since last review (Apr 3):**
1. ✅ Configurable alert thresholds for correlation breaks + scenario probability shifts (#572) — addresses Medium-Timeframe Apr 3
2. ✅ Portfolio snapshot alias for portfolio status (#575) — addresses Evening Analysis Apr 3
3. ✅ Correlation break historical context + confirmation tracking (#588) — addresses Low-Timeframe Apr 3
4. ✅ N+1 fix in movers command with batch history fetching (#590) — performance
5. ✅ N+1 fix in load_or_compute_snapshots with batch snapshot fetching (#593) — performance (brief, summary, scan, watchlist)

**Release status:** v0.25.0 tagged Apr 2. 19 feat/fix commits since tag. No P0 bugs. 2435 tests passing, clippy clean.

**GitHub stars:** 9 — Homebrew Core requires 50+.
