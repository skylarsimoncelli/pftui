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
pftui analytics macro cycles                                   # current cycle stages (existing, unchanged)
pftui analytics macro cycles history                           # composite scores, all powers, all decades (default view)
pftui analytics macro cycles history country US                # all determinants for one power
pftui analytics macro cycles history country US China          # two powers compared
pftui analytics macro cycles history metric trade              # one determinant, all powers
pftui analytics macro cycles history metric military US China  # one determinant, specific powers
pftui analytics macro cycles history decade 1940               # all powers at one point in time
pftui analytics macro cycles history composite                 # composite trajectories (same as default)
```

CRUD for populating:
```
pftui analytics macro cycles history add country US metric trade decade 1940 score 7.5 notes "..." source "..."
pftui analytics macro cycles history add-batch file data.csv
```

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
