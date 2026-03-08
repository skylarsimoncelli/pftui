# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Completed items go in CHANGELOG.md only — do not mark [x] here.

---

## P0 — Brave Search API Integration

> **Goal:** Native Brave Search API support as a single reliable data source that replaces broken scrapers and shallow RSS feeds. Optional API key — pftui works without it, but with a key the data quality jumps dramatically. Free tier gives $5/month in credits (~2000 queries), more than enough for pftui's use case.
>
> **Why:** 4 data integrations are broken (COT, BLS, on-chain, ETF flows). RSS gives headlines but no context. Brave Search API solves both problems with one integration — it can answer ANY financial question via web + news search, returning structured results with full descriptions. Instead of maintaining 10 fragile scrapers, maintain 1 reliable API client.
>
> **API:** Web search (`/res/v1/web/search`) + News search (`/res/v1/news/search`). Auth via `X-Subscription-Token` header. Free tier: $5/month auto-credited, 50 qps. News returns up to 50 results with descriptions, extra_snippets, freshness filtering.

### F26: Brave API Configuration & Client

### F27: Brave-Powered News (Replaces/Supplements RSS)

> When Brave key is configured, news comes from targeted Brave News Search queries instead of generic RSS polling. This gives article summaries (descriptions), not just headlines. Multiple focused queries replace one shallow RSS poll.

### F28: Brave-Powered Economic Data

> Instead of fragile scrapers for BLS/Trading Economics, use Brave Web Search to find latest economic readings. More resilient — when a scraper breaks because a page changed layout, Brave still works because it searches the entire web.

### F29: Brave-Powered Research Command

> A new `pftui research` command — the agent's Swiss Army knife. Instead of falling back to their own web_search tool, agents stay in pftui for ANY financial question.

### F30: Enhanced Refresh & Brief with Brave

> `pftui refresh` becomes a one-command intelligence operation. `pftui brief --agent` becomes the one JSON blob an agent needs.

---

## P0 — QA Bugs (from 2026-03-06 QA Report)

> Source: Opus QA agent ran 52 manual tests + 1105 unit tests. Full report: `QA-REPORT.md`

### Critical

### Significant

### Minor


---

## P0 — Bugs & Fixes

> Other broken functionality. Fix before shipping.

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

- [ ] **Sovereign holdings tracker** — CB gold (WGC), government BTC, COMEX silver. No other TUI tracks this. Files: new `data/sovereign.rs`
- [ ] **Multi-portfolio support** — Named portfolios with switching
- [ ] **Custom keybinding config** — User-configurable in config.toml
- [ ] **Sector heatmap** — Treemap-style sector performance view
- [ ] **Options chains** — If a free data source exists

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

1. **Brave Search API integration** (P0) — Unifies broken data pipelines (predictions, news depth, economic data) behind one reliable API. The single highest-leverage infrastructure investment. 4 data integrations still intermittent.
2. **Fix `pftui sector` data bug** (P1) — Only returns 1 of 18 ETFs. Quick win that directly improves Eventuality Planner scores. Likely a batch Yahoo Finance fetch issue.
3. **Config discoverability** (P2) — Config command is invisible to users — needs help popup and README mention (`pftui config set brave_api_key <key>`).
