# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

_(none)_

## P2 - Coverage And Agent Consumption

- [x] ~~**Alert summary for investigation continuity** — Added `--recent` and `--recent-hours` flags to `analytics alerts list`. Merged in PR #309 (Mar 25).~~
- [Feedback] **Weekend/after-hours CLI mode** — Low-Timeframe Analyst (Mar 21) suggests streamlining commands for non-market hours to skip stale intraday data and focus on positioning/prep.
- [Feedback] **Regime transition alerts on scenario probability shifts** — Medium-Timeframe Analyst (Mar 25) wants alerts when scenario probabilities shift >10% in a single session. Could integrate into the situation indicator system or create a new alert kind.

## P3 - Long Term

- **F54: Dixon Power Flow Tracker** — Track power flows between Financial Industrial Complex (FIC), Military Industrial Complex (MIC), and Technical Industrial Complex (TIC) based on Simon Dixon's "follow the money" framework. This is a new analytical layer that classifies geopolitical events by which power complex gains or loses.

  **Database table: `power_flows`**
  ```sql
  CREATE TABLE power_flows (
      id              INTEGER PRIMARY KEY AUTOINCREMENT,
      date            TEXT NOT NULL,          -- ISO date (YYYY-MM-DD)
      event           TEXT NOT NULL,          -- what happened
      source_complex  TEXT NOT NULL,          -- 'FIC', 'MIC', or 'TIC'
      direction       TEXT NOT NULL,          -- 'gaining' or 'losing'
      target_complex  TEXT,                   -- which complex is losing/gaining relative to source (nullable)
      evidence        TEXT NOT NULL,          -- the market/money signal that supports this classification
      magnitude       INTEGER NOT NULL CHECK(magnitude BETWEEN 1 AND 5), -- significance of this power shift
      agent_source    TEXT,                   -- which timeframe agent logged this (low-agent, medium-agent, etc.)
      created_at      TEXT NOT NULL DEFAULT (datetime('now'))
  );
  CREATE INDEX idx_power_flows_date ON power_flows(date);
  CREATE INDEX idx_power_flows_complex ON power_flows(source_complex);
  ```

  **CLI commands:**
  ```
  pftui analytics power-flow add --event "..." --source FIC --direction gaining --target MIC --evidence "..." --magnitude 3 [--agent-source low-agent] [--date YYYY-MM-DD]
  pftui analytics power-flow list [--complex FIC|MIC|TIC] [--days 30] [--direction gaining|losing] [--json]
  pftui analytics power-flow balance [--days 30] [--json]
  ```

  **`power-flow add`**: Creates a new power flow entry. `--source` and `--direction` are required. `--target` is optional (sometimes a complex gains without a clear loser). `--magnitude` defaults to 3 if omitted. `--date` defaults to today. Validate `--source` and `--target` are one of FIC, MIC, TIC.

  **`power-flow list`**: Lists power flow entries. Default: last 7 days. Filterable by complex, direction, days. Output format follows existing `analytics alerts list` pattern.

  **`power-flow balance`**: Aggregates power flows over the specified period and shows a net score per complex. For each complex, sum `magnitude` where `direction=gaining` minus sum `magnitude` where `direction=losing`, considering both `source_complex` and `target_complex` entries. Display as:
  ```
  POWER BALANCE (last 30 days)
  FIC:  +12 (8 gaining, 3 losing)
  MIC:  -7  (2 gaining, 6 losing)
  TIC:  +3  (4 gaining, 2 losing)
  ```
  JSON output: `{"period_days": 30, "balances": [{"complex": "FIC", "net": 12, "gaining_count": 8, "losing_count": 3, "gaining_magnitude": 15, "losing_magnitude": 3}, ...]}`

  **Agent integration**: All timeframe agents should log power flow entries when they classify events through the Dixon lens. The evening analyst synthesizes the daily balance. The morning brief includes a one-line power balance summary.

  **Design notes**: Follows existing pftui patterns — SQLite TEXT storage for dates, `--json` on every command, hierarchical CLI under `analytics`. Similar in scope to `analytics situation update log` but tracks a different dimension (power structure vs event narrative).

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
- Tests: 1628 passing (up from 1626), clippy clean
- Evening Analyst improved 72→78 usefulness, 74→75 overall — situation/narrative/deltas praised but predictions/situation-list discoverability hurt
- Medium-Timeframe stable at 85/80 — COT extreme detection was critical for repositioning calls
- Low-Timeframe Analyst recovered to 85/80 from 75/85 crash
- Alert Investigator stable and consistent across multiple daily reviews

**Top 3 priorities based on feedback:**
1. ~~**P1: `analytics predictions` discoverability**~~ — SHIPPED #300. `analytics predictions` now aliases `data predictions`.
2. ~~**P1: `analytics situation list` empty with no guidance**~~ — SHIPPED #300. JSON output now returns structured object with hint.
3. **P2: Regime transition alerts** — Medium-Timeframe Analyst wants alerts on scenario probability shifts >10%.

**Release eligibility:** v0.17.0 released. All P1 items shipped (#300 — analytics predictions alias + situation list guidance). Tests: 1628 passing, clippy clean, no P0 bugs. No P1 remaining.

**GitHub stars:** 5 — Homebrew Core requires 50+.
