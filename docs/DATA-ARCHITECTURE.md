# DATA-ARCHITECTURE.md — the layer model

> **READ THIS BEFORE adding any table or any command that stores data.**
> Every table the code creates must be classified in
> [`docs/db-catalog.toml`](db-catalog.toml). This is machine-enforced by
> `tests/schema_conformance.rs` — an unclassified table fails CI.

pftui's database accumulated 120 tables through context-free agent runs, each
bolting on a new table per TODO item. Parts go stale and infect downstream
loops. This document makes the architecture explicit: five layers, each with
one defining property, plus a DEAD bucket. The catalog is the single source
of truth for which table lives where.

## The layers

```
            external sources (Yahoo, CoinGecko, CFTC, BLS, RSS, Polymarket, ...)
                                      |
                                      v
  +------------------------------------------------------------------+
  | L0  INGEST — raw, source-stamped fetches                         |
  |     *_cache tables, economic_data, prediction_market_contracts   |
  |     Defining property: a fetch wrote it, freshness SLA applies,  |
  |     QUARANTINE lives here (economic_data.quarantined)            |
  +------------------------------------------------------------------+
                                      |
                                      v
  +------------------------------------------------------------------+
  | L1  CANONICAL SERIES — catalogued time series                    |
  |     price_history, sentiment_history, real_yields_history        |
  |     Defining property: THE durable series downstream layers      |
  |     trust; freshness SLA applies; one canonical home per series  |
  +------------------------------------------------------------------+
                                      |
                                      v
  +------------------------------------------------------------------+
  | L2  DERIVED — deterministically rebuildable from L1              |
  |     technicals, levels, signals, correlations, regime,           |
  |     calibration matrix, portfolio/position snapshots             |
  |     Defining property: a CACHE, not state — dropping it loses    |
  |     nothing; `rebuildable = true` is mandatory in the catalog    |
  +------------------------------------------------------------------+
                                      |
              +-----------------------+----------------------+
              v                                              v
  +---------------------------------+    +---------------------------------+
  | L3  LEDGERS — system outputs    |    | L4  KNOWLEDGE — curated         |
  |   analyst views, predictions,   |    |   thesis, standing rules,       |
  |   recommendations, scenarios,   |    |   lessons, sources, dossiers,   |
  |   journal, run_health           |    |   watchlist/config              |
  |   Defining property: append-    |    |   Defining property: human/     |
  |   only and SCOREABLE — never    |    |   agent-curated, review-dated,  |
  |   rewritten, only scored        |    |   small, slow-changing          |
  +---------------------------------+    +---------------------------------+
              |                                              |
              +-----------------------+----------------------+
                                      v
                       reports, briefs, newsletter, TUI/web
```

DEAD: no code writer AND no code reader (grep `src/`), or zero rows with no
writer. DEAD tables stay in the catalog (so the conformance test still
passes) until R3 archives or drops them.

## The rules

1. **No new table without a catalog entry.** Adding a table means adding a
   `[tables.<name>]` entry to `docs/db-catalog.toml` in the same commit, with:
   - `layer` — one of `L0 L1 L2 L3 L4` (never DEAD at birth)
   - `purpose` — one honest line
   - `writers` / `readers` — the src files that touch it
   - a **named consumer**: something must read it. Write-only ingestion is
     wasted ingestion (see `narrative_money_history` in the census).
   - the layer's property: `freshness_sla_hours` (L0/L1),
     `rebuildable = true` (L2), `append_only = true` (L3 ledgers)
2. **L2 must be rebuildable.** If you cannot regenerate the table from L0/L1
   with a deterministic function, it is not L2 — it is probably L3.
3. **L3 is never mutated.** Heads (e.g. `scenarios`, `analyst_views`) change
   only through guarded update paths that write a ledger row
   (`scenario_updates`, `analyst_view_history`). History/ledger tables are
   append-only; scoring fills outcome fields, nothing else is rewritten.
4. **One canonical home per series.** Don't add a second table for a series
   that already has an L1 home; extend the existing one.
5. **Classify by primary role.** When a table straddles layers, pick the
   primary role and note the ambiguity in `purpose`.
6. **TODO items that add storage are capability briefs**, not table names:
   they must name the layer, the contract (SLA / rebuild function /
   append-only), and the consumer that will read the data.

## How it is enforced

`tests/schema_conformance.rs` (runs in `cargo test`):

1. Builds a fresh DB through the real migration path (drives the binary in an
   isolated HOME, reads `system db-info --json`) and asserts every migrated
   table has a catalog entry.
2. Scans `src/**/*.rs` for `CREATE TABLE` statements (test modules excluded)
   so lazily-created tables (`analyst_views`, `mirror_sync_state`, ...) are
   covered too.
3. Validates every entry: valid layer, non-empty purpose, writers/readers
   arrays, `rebuildable = true` on every L2 table.
4. Reverse direction: every catalog entry must correspond to a table the code
   actually creates — the catalog cannot drift into fiction.

A new table without an entry fails CI with a message pointing here.

## Census summary (2026-06-11)

Full machine-readable detail in `docs/db-catalog.toml`; regenerate raw census
data with `scripts/db_census.py` (metadata-only: names, schemas, rowcounts,
MAX(timestamp) — never row contents).

| Layer | Tables | Notes |
|---|---|---|
| L0 ingest | 19 | 3 dormant scaffolds (capital_flows, options_chain_snapshots, prediction_cache→DEAD) |
| L1 canonical series | 4 | price_history, sentiment_history, real_yields_history, predictions_history (dormant) |
| L2 derived | 20 | includes the two largest tables (technical_snapshots 1.4M, correlation_snapshots 1.3M rows) |
| L3 ledgers | 42 | largest layer — every epistemic mechanism ships its own ledger |
| L4 knowledge | 32 | thesis/lessons/rules + operator config (watchlist, groups, alerts) |
| DEAD | 3 | see below |
| **Total** | **120** | |

**DEAD list** (R3 to archive/drop/migrate):

| Table | Rows | Why dead | Recommendation |
|---|---|---|---|
| `prediction_cache` | 0 | 0 external call sites; superseded by `predictions_cache` | drop module + table |
| `conviction_durability` | 15 | no code writer or reader; rows via agent raw SQL | migrate to a code path or drop |
| `thesis_citations` | 5,136 | no code writer or reader; rows via agent raw SQL | adopt with a code path or archive |

**Legacy list** (live DB but not code-created): **none** — every table in the
live DB is created by code. The drift problem here is staleness and
classification, not orphaned schema.

**Notable findings from the census:**

- `run_health` has **0 rows** — the EPISTEMICS instrumentation spine has
  never been recorded on this machine.
- `narrative_money_history` is write-only: 107 rows ingested, no production
  reader. Wasted ingestion until a reader ships.
- `scenario_prediction_links` (3,416 rows) has readers but **no code
  writer** — populated via agent raw SQL; needs a writer command.
- `cot_cache` and `futures_cache` last wrote 2026-05-25 — COT is past its
  192h SLA (weekly report; ~17 days stale at census time).
- 22 tables have 0 rows; most are wired scaffolds awaiting their feature
  (`recommendation_outcomes`, `news_source_accuracy`, `alignment_score_history`,
  `dividends`, `gex_snapshots`, ...). Empty + wired ≠ DEAD, but each one is a
  promised loop that has never closed.

## Research harness (R1a) — the L2 expectancy flow

The research harness (`src/research/`) is the first module built ON the layer
model rather than retrofitted into it:

```
price_history (L1)
      |
      v
AssetContext::build  — one pass per asset: market-structure walks (daily +
      |                weekly), one Cyber engine run, one cycle-engine run,
      |                SMA200/RSI14 (src/research/registry.rs)
      v
signal emitters      — ~27 canonical (id, version, description, emitter)
      |                rows; dated EVENTS = state transitions, never states
      v
event_study::study   — forward returns at 5/30/90/180d vs the asset's own
      |                baseline drift; MAE/MFE; overlap exclusion; exact
      |                binomial significance vs the baseline up-rate; era +
      |                200dma-regime splits (src/research/event_study.rs)
      v
signal_expectancy (L2, rebuildable = true)
      |                PK (signal_id, signal_version, asset, horizon, as_of)
      v
readers: `pftui research expectancy/events`, the report per-asset card's
"Signal expectancy" line (fires-in-last-10-days citation)
```

Layer contract: `signal_expectancy` is a CACHE — dropping it loses nothing;
`pftui research backtest` rebuilds it deterministically from `price_history`.
Walk-forward discipline: emitters date events at the bar where the transition
became observable; `study()` only admits events `<= as_of` whose forward
window fully resolved by `as_of`; persisted rows carry that `as_of`, so a
report citation can never lean on data that did not exist. Emitter logic
changes MUST bump the signal's version string — stats bind to
`(signal_id, signal_version)` and stale stats are never cited against a
changed definition. Documented parameter-snapshot exceptions (cycle
timing-band percentiles and FLD offset come from the engine's
as-of-truncated full-sample stats) live in the `registry.rs` module docs.
