# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 — Feature Requests

### F39: Macro Analytics Consolidation (`pftui analytics macro`)

> Collapse `pftui structural` into `pftui analytics macro`. Every analytical view
> lives under one namespace. Database schema unchanged. CLI routing + computation layer.

**F39.5: Fourth Turning cycle enrichment**

Extend `structural_cycles` to support richer metadata for Fourth Turning tracking.
Add optional JSON `metadata` column (or use existing `evidence` text field) to store:
- `turning_number` (1-4)
- `phase` (catalyst / regeneracy / climax / resolution)
- `catalyst_event`, `estimated_climax_range`, `key_institutions`

CLI: `pftui analytics macro cycles update "Fourth Turning" --phase climax --evidence "..."`


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
