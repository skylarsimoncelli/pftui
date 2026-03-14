# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 — Feature Requests

### F39: Macro Analytics Consolidation

Routing and composite score shipped. Remaining:

**F39.7: Empire Cycle History (`pftui analytics macro cycles history`)**

New table `power_metrics_history`:
```sql
CREATE TABLE power_metrics_history (
  id BIGSERIAL PRIMARY KEY,
  country TEXT NOT NULL,
  metric TEXT NOT NULL,
  decade INTEGER NOT NULL,
  score DOUBLE PRECISION NOT NULL,
  notes TEXT,
  source TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(country, metric, decade)
);
CREATE INDEX idx_pmh_country ON power_metrics_history(country);
CREATE INDEX idx_pmh_decade ON power_metrics_history(decade);
```

Separate from live `power_metrics` because historical scores are immutable reference data.
Live metrics change weekly; 1940 US military score never changes.

CLI subcommands (nested under existing `cycles`):
```
pftui analytics macro cycles                                                          # current cycle stages (existing, unchanged)
pftui analytics macro cycles history                                                  # composite scores, all powers, all decades (default)
pftui analytics macro cycles history --country US                                     # all determinants for one power
pftui analytics macro cycles history --country US --country China                     # two powers compared
pftui analytics macro cycles history --metric trade                                   # one determinant, all powers
pftui analytics macro cycles history --metric military --country US --country China   # one determinant, specific powers
pftui analytics macro cycles history --decade 1940                                    # all powers at one point in time
pftui analytics macro cycles history --composite                                      # composite trajectories (same as default)
```

CRUD for populating:
```
pftui analytics macro cycles history add --country US --metric trade --decade 1940 --score 7.5 --notes "..." --source "..."
pftui analytics macro cycles history add-batch --file data.csv
```

CLI design rule: functions are commands (navigation tree). Parameters are arguments (--flags).
See PRODUCT-PHILOSOPHY.md principle 9.

Display format (for `--composite`):
```
        1900  1910  1920  1930  1940  1950  1960  1970  1980  1990  2000  2010  2020  2026
US       6.2   7.0   7.8   7.5   8.5   9.5   9.5   9.0   8.5   9.0   9.0   8.5   8.0   7.6
UK       9.0   8.5   7.5   7.0   7.0   6.0   5.0   4.5   4.5   4.5   4.5   4.0   3.5    —
China    3.0   2.5   2.0   2.0   2.5   3.0   3.5   3.5   4.0   5.0   5.5   6.0   6.5   6.1
Russia   5.0   5.0   3.0   4.5   7.0   8.0   8.5   8.0   7.0   4.0   3.5   4.0   4.5   4.3
```

2026 column pulls from live `power_metrics` table (computed composite, not stored in history).

Powers to track: US, China, Russia/USSR, UK/British Empire, EU (from 1950),
India (from 1950), Saudi (from 1940), Japan.
UK and Japan are not in live tracker but essential for historical narrative.
UK = empire decline case study. Japan = post-peak stagnation case study.

9 determinants x 8 powers x ~12 decades = ~700 rows.

Source: `src/commands/analytics.rs`, `src/db/structural.rs`

### F40: CLI Hierarchy Restructure

> Collapse 55 top-level commands into ~15 with logical groupings.
> Old commands become deprecated aliases (print warning, forward to new path).
> No functionality changes. Pure routing refactor.
> See PRODUCT-PHILOSOPHY.md principle 9 and CLAUDE.md CLI Design Rules.

**F40.1: `pftui portfolio` namespace**

Move portfolio-related commands under `portfolio`:
```
pftui portfolio summary         # was: pftui summary
pftui portfolio value           # was: pftui value
pftui portfolio performance     # was: pftui performance
pftui portfolio history         # was: pftui history
pftui portfolio brief           # was: pftui brief
pftui portfolio eod             # was: pftui eod
pftui portfolio target          # was: pftui target
pftui portfolio drift           # was: pftui drift
pftui portfolio rebalance       # was: pftui rebalance
pftui portfolio stress-test     # was: pftui stress-test
pftui portfolio dividends       # was: pftui dividends
pftui portfolio annotate        # was: pftui annotate
pftui portfolio group           # was: pftui group
pftui portfolio opportunity     # was: pftui opportunity
pftui portfolio set-cash        # was: pftui set-cash
```
`pftui portfolio` with no subcommand shows `summary` (default view).
Source: `src/cli.rs` (add Portfolio subcommand enum), `src/main.rs` (dispatch).
Keep old top-level commands as deprecated aliases.

**F40.2: `pftui transaction` namespace**

Consolidate transaction CRUD:
```
pftui transaction add            # was: pftui add-tx
pftui transaction remove         # was: pftui remove-tx
pftui transaction list           # was: pftui list-tx
```
Source: `src/cli.rs`, `src/main.rs`, `src/commands/transactions.rs`.
Alias `add-tx` → `transaction add`, etc.

**F40.3: `pftui watchlist` consolidation**

Merge three commands into one:
```
pftui watchlist add AAPL         # was: pftui watch AAPL
pftui watchlist remove AAPL      # was: pftui unwatch AAPL
pftui watchlist list             # was: pftui watchlist (unchanged, becomes default)
```
`pftui watchlist` with no subcommand shows `list`.
Source: `src/cli.rs`, `src/main.rs`.

**F40.4: `pftui market` namespace**

Group external market data commands:
```
pftui market news                # was: pftui news
pftui market sentiment           # was: pftui sentiment
pftui market calendar            # was: pftui calendar
pftui market fedwatch            # was: pftui fedwatch
pftui market economy             # was: pftui economy
pftui market predictions         # was: pftui predictions (Polymarket)
pftui market options             # was: pftui options
pftui market etf-flows           # was: pftui etf-flows
pftui market supply              # was: pftui supply
pftui market sovereign           # was: pftui sovereign
```
Source: `src/cli.rs`, `src/main.rs`.

**F40.5: `pftui dashboard` namespace**

Group pre-built dashboard views:
```
pftui dashboard macro            # was: pftui macro
pftui dashboard oil              # was: pftui oil
pftui dashboard crisis           # was: pftui crisis
pftui dashboard sector           # was: pftui sector
pftui dashboard heatmap          # was: pftui heatmap
pftui dashboard global           # was: pftui global
```
Source: `src/cli.rs`, `src/main.rs`.

**F40.6: `pftui system` namespace**

Group admin/system commands:
```
pftui system config              # was: pftui config
pftui system db-info             # was: pftui db-info
pftui system doctor              # was: pftui doctor
pftui system export              # was: pftui export
pftui system import              # was: pftui import
pftui system snapshot            # was: pftui snapshot
pftui system setup               # was: pftui setup
pftui system demo                # was: pftui demo
pftui system web                 # was: pftui web
pftui system status              # was: pftui status
pftui system migrate-journal     # was: pftui migrate-journal
```
Source: `src/cli.rs`, `src/main.rs`.

**F40.7: Move `movers` and `correlations` under `analytics`**

```
pftui analytics movers           # was: pftui movers
pftui analytics correlations     # was: pftui correlations
```
These are analytical views, not standalone tools.
Source: `src/cli.rs`, `src/commands/analytics.rs`.

**F40.8: Convert positional `<ACTION>` to proper clap subcommands**

These commands use `<ACTION>` as a positional string argument. Convert to proper
clap `Subcommand` enums so each action gets its own `--help` with only relevant flags:

- `scenario` (add, list, update, remove, signal-add, signal-list, signal-update, signal-remove, history)
- `predict` (add, list, score, stats, scorecard)
- `conviction` (set, list, history, changes)
- `trends` (add, list, update, evidence-add, evidence-list, impact-add, impact-list, dashboard)
- `notes` (add, list, search, remove)
- `alerts` (add, list, remove, check, ack, rearm)
- `journal` (add, list, search, update, remove, tags, stats)
- `agent-msg` (send, list, ack)
- `regime` (current, history, transitions)
- `analytics` (summary, low, medium, high, macro, alignment, divergence, digest, recap, gaps, signals)

Currently `pftui scenario --help` shows ALL flags for ALL actions. After this change,
`pftui scenario add --help` shows only add-relevant flags.
Source: `src/cli.rs` (refactor each into nested Subcommand enum), `src/main.rs` (dispatch).

**F40.9: Deprecated alias system**

All old top-level commands must continue working with a deprecation warning:
```
$ pftui macro
Warning: `pftui macro` is deprecated. Use `pftui dashboard macro` instead.
[normal output follows]
```
Implementation: match old command names in `src/main.rs`, print warning to stderr,
forward to new dispatch. Remove aliases after 3 major versions.
`structural` already uses this pattern — reuse the same mechanism.

**Final top-level tree after F40:**
```
pftui
├── portfolio        # holdings, value, performance, targets, rebalancing
├── transaction      # add, remove, list
├── watchlist        # add, remove, list
├── analytics        # multi-timeframe engine, movers, correlations
├── market           # news, sentiment, calendar, fedwatch, economy, etc.
├── dashboard        # macro, oil, crisis, sector, heatmap, global
├── scenario         # scenario tracking (proper subcommands)
├── predict          # prediction tracking (proper subcommands)
├── conviction       # conviction scoring (proper subcommands)
├── trends           # structural trends (proper subcommands)
├── journal          # trade journal (proper subcommands)
├── notes            # research notes (proper subcommands)
├── alerts           # price/allocation alerts (proper subcommands)
├── agent-msg        # inter-agent messaging (proper subcommands)
├── regime           # market regime (proper subcommands)
├── scan             # position scanner
├── research         # Brave search
├── refresh          # data refresh
├── system           # config, doctor, export, import, setup, demo, web
└── help
```
55 top-level → 19. Every grouping navigable via `--help`.

### Alignment Scoring Algorithm

Current alignment score (5.6%) is too basic. Need per-asset alignment score (0-100)
weighting: conviction score, trend direction, regime state, scenario probability impact.
This IS the deployment signal tracker. Must be pftui's best feature.

### Data Source Reliability

8/10 sources stale, price_history writes stopped. Must stabilize.

---

## P2 — Nice to Have

- Prediction resolution criteria column + CLI flag
- `pftui scan --news-keyword` flag for news_cache matching
- Brief movers scope: show market-wide movers, not just portfolio

---

## P3 — Long Term

### F36: Investor Perspectives Panel

Multi-lens macro analysis via sub-agents. 15 named legends + 10 archetypes + custom.
Full spec in git history (commit `5e34607`). Depends on F31 `--json` completeness
and OpenClaw sub-agent spawning.

### F39.7 Data Population (Sentinel, post-dev-cron)

> After dev cron ships F39.7 CLI + schema, spawn a research sub-agent to populate
> the historical database. The sub-agent should:
>
> 1. Research each determinant for each power at each decade using web_search
> 2. Score on Dalio's 1-10 scale with brief justification and source
> 3. Populate via `pftui analytics macro history add` CLI commands
> 4. Cross-reference Dalio's own charts from "Principles for Dealing with
>    the Changing World Order" as a baseline, then refine with primary sources
>
> Powers and spans:
> - US: 1900-2020 (13 decades)
> - China: 1900-2020 (13 decades)
> - Russia/USSR: 1900-2020 (13 decades, note regime transitions)
> - UK/British Empire: 1900-2020 (13 decades, the decline narrative)
> - Japan: 1900-2020 (13 decades, rise and plateau)
> - EU: 1950-2020 (8 decades, post-ECSC)
> - India: 1950-2020 (8 decades, post-independence)
> - Saudi: 1940-2020 (9 decades, post-oil discovery)
>
> Estimated: ~700 rows. Each needs a score, notes, and source.
> Break into multiple sub-agent runs by country if needed.
