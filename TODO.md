# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Completed items go in CHANGELOG.md only — do not mark [x] here.

---

## P1 — Feature Requests

> User-requested features and high-value improvements.

### Data & Display

### CLI Enhancements

### Analytics

### Infrastructure
- [ ] **PostgreSQL backend support** — Add PostgreSQL as alternative to SQLite via `sqlx` (runtime, not compile-time). `pftui setup` already handles DB choice — add Postgres option to the wizard. Migration uses existing workflow: `pftui export json` → `pftui setup` (pick new backend) → `pftui import`. Files to change:
  - `db/backend.rs` (new) — abstraction layer over `rusqlite`/`sqlx`
  - `db/schema.rs` + `db/*.rs` — abstract all queries to work with both backends
  - `config.rs` — `database.backend` + `database.url` fields
  - `commands/setup.rs` — add Postgres option to wizard
  - `docs/MIGRATING.md` (new) — document the 3-step export/setup/import workflow
  - `README.md` — add "SQLite (default) or PostgreSQL" to features, install section
  - `website/index.html` — update comparison table + features to mention Postgres support
  - `AGENTS.md` — update data model section to explain both backends + how agents should handle it

---

## P2 — Nice to Have

> Future improvements. Lower priority.

### TUI Polish (batch: ~4hrs total)

### Watchlist (batch: ~2hrs total)

### Scanner (batch: ~3hrs total)

### Distribution
- [ ] **Snap/AUR/Scoop publishing** — Needs accounts + secrets for each store
- [ ] **Homebrew Core** — Needs 50+ GitHub stars (currently 1)

### Other

---

## P3 — Long Term


---

## Feedback Summary

> Updated: 2026-03-08

### Current Scores (latest per tester)

| Tester | Usefulness | Overall | Trend |
|--------|-----------|---------|-------|
| Market Research | 88% | 82% | ↑ (40→72→78→78→74→88) |
| Eventuality Planner | 82% | 80% | ↑ (38→85→92→85→80→82) |
| Sentinel (Portfolio Analyst) | 85% | 88% | ↑ (78→82→82→78→82→88) |
| Market Close | 92% | 88% | ↑ (68→80→72→88) |
| UX Analyst | — | 75% | → (78→68→72→73→75) |

### Score Trends

- **Market Research:** Strong upswing to 88/82 — best scores yet. Macro technicals (RSI/MACD/SMA) landed on Mar 7 and this tester noticed. Remaining gap: oil technicals in brief (now in macro), and prediction markets showing sports instead of geopolitical. Python script dependency nearly eliminated.
- **Eventuality Planner:** Stable at 82/80. `eod` command and macro dashboard are star features. Pain points: sector command returning only 1 ETF, prediction markets filtering for geopolitics, and missing ag commodity tracking. Wants CME FedWatch.
- **Sentinel (Portfolio Analyst):** Best overall score yet (85/88). TUI visual quality consistently praised. Ratio charts context header (added Mar 7) well received.
- **Market Close:** Strongest absolute scores (92/88) — no new review since Mar 6. `brief + movers + macro` pipeline covers most of the routine. Python script nearly eliminated.
- **UX Analyst:** Slight uptick to 75. Focus shifted from CLI consistency (mostly fixed) to feature discoverability (`pftui config` invisible) and `status --json` gap. Data pipeline reliability improving but predictions/COT still intermittent.

### Top 3 Priorities (Feedback-Driven)

1. ✅ **Brave Search API integration** — COMPLETE (Mar 7, 2026). Config, client, news, economic data, research command all shipped.
2. **Config discoverability** — Config command exists but isn't surfaced in help or README.
3. **PostgreSQL backend support** — The only remaining P1 item.
