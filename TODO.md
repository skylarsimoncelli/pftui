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
| Evening Analyst | 72% | 75% | Mar 27 | ↑ (recovered from 65/68 Mar 26. Journal entry UX fix #375 shipped. Economy confidence still a pain point. **Lowest scorer — priority.**) |
| Medium-Timeframe Analyst | 85% | 88% | Mar 27 | ↑ (overall up 80→88. Power flow assess #372 shipped for FIC/MIC/TIC weekly tracking.) |
| Low-Timeframe Analyst | 85% | 90% | Mar 27 | ↑ (recovered from 75/80. Synthesis power structure integration #384 shipped. Regime-flows #369, prediction stats filters #356, news sentiment #358 all shipped.) |
| High-Timeframe Analyst | 85% | 90% | Mar 26 | → (stable. Scenario suggest #366 shipped.) |
| Morning Brief | 85% | 88% | Mar 26 | → (stable. Consolidated morning-brief #363 shipped.) |
| Alert Investigator | 85% | 80-82% | Mar 25-26 | → (stable, consistent. System healthy.) |
| Dev Agent | 92% | 94% | Mar 27 | → (stable high.) |

**Key changes since last review (Mar 26):**
- v0.18.0 was released Mar 26. 61 commits since tag.
- **v0.19.0 released Mar 27** with 15 feature PRs since v0.18.0.
- Evening Analyst **recovered** 65→72 usefulness, 68→75 overall — journal UX fix helped, economy confidence still limiting.
- Medium-Timeframe Analyst **up** 80→88 overall — power flow weekly assessment praised.
- Low-Timeframe Analyst **recovered** from 75/80 dip, back to 85/90.
- All other testers stable.

**Shipped since last review (Mar 26):**
1. ✅ **Synthesis power structure integration** — #384. `analytics synthesis --json` now includes FIC/MIC/TIC context.
2. ✅ **Economy indicator confidence depth** — #381. FRED 5→15 indicators, confidence_reason, previous/change.
3. ✅ **Power flow weekly assessment** — #372. `analytics power-flow assess` for FIC/MIC/TIC tracking.
2. ✅ **Regime-asset flow correlation tracker** — #369. `analytics regime-flows --json`.
3. ✅ **Automated scenario probability suggestions** — #366. `analytics scenario suggest --json`.
4. ✅ **Consolidated morning-brief command** — #363. `analytics morning-brief --json`.
5. ✅ **News sentiment scoring** — #358. `analytics news-sentiment` + `data news --with-sentiment`.
6. ✅ **Prediction stats per-timeframe/agent filtering** — #356. `--timeframe`/`--agent` on stats.
7. ✅ **Alignment summary** — #353. `analytics alignment --summary`.
8. ✅ **Sector-wide theme detection** — #351. `analytics movers themes`.
9. ✅ **Auto-scored predictions** — #341. `journal prediction auto-score`.
10. ✅ **Correlation breaks × impact cross-ref** — #341. `--with-impact` on correlations.
11. ✅ **Data futures endpoint** — #340. `data futures` for overnight positioning.
12. ✅ **Economy confidence + scenario discoverability** — #339. FRED thresholds, CPI/PPI mappings, scenario plural alias.
13. ✅ **Journal entry add UX fix** — #375. `--content` named flag, help text on all flags.

**Release:** v0.19.0 cut Mar 27 — 61 commits, 15 features, 1787 tests, clippy clean.

**GitHub stars:** 6 — Homebrew Core requires 50+.
