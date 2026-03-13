# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 — Feature Requests

### F39: Macro Analytics Consolidation (`pftui analytics macro`)

> Collapse `pftui structural` into `pftui analytics macro`. Every analytical view
> lives under one namespace. Database schema unchanged. CLI routing + computation layer.

**F39.1: Route `analytics macro` subcommands (rename)**

Reroute all `structural` subcommands under `analytics macro`:
```
pftui analytics macro                    # dashboard (default view)
pftui analytics macro metrics US         # Dalio 8 determinants for country
pftui analytics macro metrics China      # same
pftui analytics macro compare US China   # side-by-side power comparison
pftui analytics macro cycles             # Big Cycle + Fourth Turning stages
pftui analytics macro outcomes           # structural outcome probabilities
pftui analytics macro parallels          # historical parallel tracker
pftui analytics macro log                # weekly structural observations
```
Keep `pftui structural` as a deprecated alias (prints warning + forwards).
Source: `src/commands/analytics.rs` (add macro subcommand dispatch),
`src/commands/structural.rs` (existing logic, reuse).

**F39.2: Dalio composite score computation**

`pftui analytics macro metrics US` should compute and display a weighted composite
score (0-10) from the 8 Dalio determinants: education, innovation, competitiveness,
military, trade, economic output, financial center, reserve currency.
Show: individual scores with trend arrows, composite at bottom, delta from last update.
Source: `src/db/structural.rs` (add composite query), `src/commands/analytics.rs`.
Tables: `power_metrics` (existing). Default weights: equal (1/8 each).

**F39.3: Country-filtered metric list**

`pftui analytics macro metrics --country US` returns ONLY that country's metrics.
Currently `metric-list` dumps all countries mixed together. Add `--country` filter.
Source: `src/commands/structural.rs` (add WHERE clause on country).

**F39.4: Head-to-head power comparison**

`pftui analytics macro compare US China` shows side-by-side table:
```
Determinant      | US    | China | Gap   | Trend
Education        | 7.5 ↓ | 7.0 ↑ | -0.5  | Closing
Innovation       | 8.0 → | 6.5 ↑ | -1.5  | Closing
Military         | 9.0 → | 6.5 ↑ | -2.5  | Closing
...
Composite        | 6.7 ↓ | 6.1 ↑ | -0.6  | Closing
```
Source: `src/commands/analytics.rs`. Gracefully handle missing metrics (show "—").

**F39.5: Fourth Turning cycle enrichment**

Extend `structural_cycles` to support richer metadata for Fourth Turning tracking.
Add optional JSON `metadata` column (or use existing `evidence` text field) to store:
- `turning_number` (1-4)
- `phase` (catalyst / regeneracy / climax / resolution)
- `catalyst_event`, `estimated_climax_range`, `key_institutions`

CLI: `pftui analytics macro cycles update "Fourth Turning" --phase climax --evidence "..."`

**F39.6: Ensure China metric parity**

US has 8+ metrics, China has only 4. Missing: competitiveness, trade, economic output,
reserve currency, governance. Comparison command (F39.4) must handle missing gracefully.

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

> Multi-lens macro analysis via sub-agents. 15 named legends + 10 archetypes + custom.
> Full spec in git history (commit `5e34607`). Depends on F31 `--json` completeness
> and OpenClaw sub-agent spawning.

### F39 Routine Integration (Sentinel, post-dev-cron)

> After dev cron ships F39, rewrite macro-timeframe-analyst routine with two explicit lenses.

- [ ] **F39.1 shipped** → Replace all `pftui structural` with `pftui analytics macro` in routines
- [ ] **F39.2 shipped** → Add composite score tracking to macro routine
- [ ] **F39.4 shipped** → Add `compare US China --json` as primary macro input
- [ ] **F39.5 shipped** → Rewrite macro routine with two analytical lenses:

  **Lens 1: Dalio Big Cycle (8 Determinants)**
  - Review all 8 determinants for US and China every run
  - Track composite score trend and gap closure rate
  - Map current Big Cycle stage (Dalio's 6 stages)
  - Key question: "Is the empire transition accelerating or decelerating?"

  **Lens 2: Strauss-Howe Fourth Turning**
  - Assess current phase (catalyst / regeneracy / climax / resolution)
  - Track crisis arc markers: institutional legitimacy, generational power transfer, external conflict
  - Historical parallel: what happened at this phase in previous Fourth Turnings?
  - Key question: "Where are we in the crisis arc and what does resolution look like?"

  Both lenses produce falsifiable observations that constrain lower timeframes.
