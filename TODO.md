# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

- [Feedback] **`analytics predictions` command missing — discoverability gap** — Evening Analyst (Mar 25) tried `analytics predictions` which doesn't exist; the correct paths are `journal prediction list` (journal predictions) and `data predictions` (Polymarket). Either add `analytics predictions` as an alias/redirect or surface a helpful error. Evening Analyst is the lowest scorer — this directly caused empty output and confusion.
- [Feedback] **`analytics situation list` returns empty with no guidance** — Evening Analyst (Mar 25) got empty `[]` from `analytics situation list` because no scenarios have been promoted to active situations. Should return a helpful message explaining that scenarios need to be promoted via `journal scenario promote` before they appear as situations.

## P2 - Coverage And Agent Consumption

- [Feedback] **Alert summary for investigation continuity** — Alert Investigator (Mar 24) wants a command showing recent acknowledged alerts to maintain investigation context across cycles. May extend `analytics alerts list --acknowledged --recent`.
- [Feedback] **Weekend/after-hours CLI mode** — Low-Timeframe Analyst (Mar 21) suggests streamlining commands for non-market hours to skip stale intraday data and focus on positioning/prep.
- [Feedback] **Regime transition alerts on scenario probability shifts** — Medium-Timeframe Analyst (Mar 25) wants alerts when scenario probabilities shift >10% in a single session. Could integrate into the situation indicator system or create a new alert kind.

## P3 - Long Term

_(none)_

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 78% | 75% | Mar 25 | ↑ (up from 72/74 on Mar 24. Situation/narrative/deltas commands praised. Still lowest scorer — `analytics predictions` and `analytics situation list` returned empty.) |
| Medium-Timeframe Analyst | 85% | 80% | Mar 25 | → (stable at 85/80. COT extreme detection praised. Wants regime transition alerts on probability shifts >10%.) |
| Low-Timeframe Analyst | 85% | 80% | Mar 24 | ↑ (recovered from 75/85 crash on Mar 23 — deltas fix helped. Wants integrated news sentiment scoring.) |
| High-Timeframe Analyst | 85% | 75% | Mar 23 | → (no new review since Mar 23. Economy data quality fixes shipped.) |
| Low-Timeframe Midday | 85% | 88% | Mar 23 | → (stable, no new review.) |
| Morning Intelligence | 85% | 90% | Mar 23 | → (stable, no new review.) |
| Alert Investigator | 85% | 80-82% | Mar 25 | → (stable, consistent routine monitoring. Multiple reviews today — system working well. Wants alert summary for continuity.) |
| Dev Agent | 92% | 94% | Mar 25 | → (stable high. Shipped correlation breaks #291 cleanly. Codebase praised as well-structured.) |

**Key changes since last review (Mar 24):**
- v0.16.0 released Mar 24. 47 new commits since tag.
- Shipped: correlation breaks in situation room (#291), correlations --json + list subcommand (#283), portfolio unrealized (#277), portfolio daily-pnl (#270), analytics situation --json fix (#263), economy data format fix (#257), mobile TLS cert fix
- Tests: 1626 passing (up from 1604), clippy clean
- Evening Analyst improved 72→78 usefulness, 74→75 overall — situation/narrative/deltas praised but predictions/situation-list discoverability hurt
- Medium-Timeframe stable at 85/80 — COT extreme detection was critical for repositioning calls
- Low-Timeframe Analyst recovered to 85/80 from 75/85 crash
- Alert Investigator stable and consistent across multiple daily reviews

**Top 3 priorities based on feedback:**
1. **P1: `analytics predictions` discoverability** — Evening Analyst (lowest scorer) hit empty output trying a nonexistent command path.
2. **P1: `analytics situation list` empty with no guidance** — Evening Analyst confusion when no scenarios are promoted.
3. **P2: Regime transition alerts** — Medium-Timeframe Analyst wants alerts on scenario probability shifts >10%.

**Release eligibility:** v0.16.0 released Mar 24. 47 new commits since tag with features (correlation breaks in situation room, correlations --json/list, portfolio unrealized, portfolio daily-pnl) and fixes (analytics situation --json, economy data format, mobile TLS cert). All tests pass (1626), clippy clean, no P0 bugs. **Ready to release v0.17.0** when the current batch stabilizes — suggest waiting for the P1 discoverability fixes to land first.

**GitHub stars:** 5 — Homebrew Core requires 50+.
