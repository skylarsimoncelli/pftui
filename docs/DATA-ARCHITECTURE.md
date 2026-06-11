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
writer. A DEAD table stays in the catalog (so the conformance test still
passes) until it is archived-then-dropped; the R3 cull (2026-06-11) cleared
the original DEAD list.

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

## The series registry (R3) — registration, not physical migration

`series_registry` is the L1 meta-table: one row per canonical time series,
naming WHERE the series physically lives (`storage_table` + `storage_filter`
+ `date_column`), its canonical symbol (and deep alias, e.g. BTC→BTC-USD),
source, units, and the freshness SLA it must meet. Seeded at migration with
the core series: the major price symbols (GC=F, SI=F, BTC, GLD, SPY, ^GSPC,
^VIX, DX-Y.NYB, ^TNX, CL=F), both Fear & Greed gauges, every plausible-range
economic indicator, BTC ETF flows, exchange reserves, and the four COT
contracts. Seeding is `INSERT OR IGNORE` — operator edits (e.g. a tightened
SLA) survive restarts.

Freshness machinery driven by the registry:

- `pftui data series status [--json]` — per-series last datapoint, age,
  staleness vs SLA (glyph table).
- `pftui system doctor` — warns, naming each series, when any registered
  series is past **2× its SLA** (a feed gone dark, not routine drift).

**Physical consolidation of the underlying tables is explicitly deferred:
registration now, physical merge when a consumer needs it.** The registry
makes "where does this series live and is it fresh?" answerable without
moving a single row; merging `sentiment_history` / `onchain_cache` / etc.
into one physical series table buys nothing until something needs to query
across them — when that consumer appears, the registry rows already define
the mapping the migration would follow.

## Census summary (2026-06-11, updated post-R3 cull)

Full machine-readable detail in `docs/db-catalog.toml`; regenerate raw census
data with `scripts/db_census.py` (metadata-only: names, schemas, rowcounts,
MAX(timestamp) — never row contents).

| Layer | Tables | Notes |
|---|---|---|
| L0 ingest | 19 | 2 dormant scaffolds (capital_flows, options_chain_snapshots) |
| L1 canonical series | 5 | price_history, sentiment_history, real_yields_history, predictions_history (dormant), series_registry (meta) |
| L2 derived | 20 | includes the two largest tables (technical_snapshots 1.4M, correlation_snapshots 1.3M rows) |
| L3 ledgers | 41 | largest layer — every epistemic mechanism ships its own ledger |
| L4 knowledge | 32 | thesis/lessons/rules + operator config (watchlist, groups, alerts) |
| DEAD | 0 | R3 cull complete — see below |
| **Total** | **117** | |

**R3 cull (2026-06-11)** — all four DEAD tables archived (JSON in
`~/pftui-archives/`, non-empty tables only) then dropped by migration; the
migration is a no-op on fresh DBs and skips the drop if the archive write
fails:

| Table | Rows at drop | Why dead |
|---|---|---|
| `prediction_cache` | 0 | superseded by `predictions_cache`; module deleted |
| `conviction_durability` | 15 | no code writer/reader (agent raw SQL); archived |
| `thesis_citations` | 5,136 | no code writer/reader (agent raw SQL); archived |
| `narrative_money_history` | 107 | write-only ingestion, no reader; archived; refresh write removed |

**Legacy list** (live DB but not code-created): **none** — every table in the
live DB is created by code. The drift problem here is staleness and
classification, not orphaned schema.

**Notable findings from the census:**

- `run_health` has **0 rows** — the EPISTEMICS instrumentation spine has
  never been recorded on this machine.
- `scenario_prediction_links` (3,416 rows) has readers but **no code
  writer** — populated via agent raw SQL; needs a writer command.
- `cot_cache` and `futures_cache` last wrote 2026-05-25 — COT is past its
  192h SLA (weekly report; ~17 days stale at census time). Now surfaced by
  the registry-driven doctor check.

## Empty scaffolds — close the loop or cull next

21 zero-row tables remain after the R3 cull (22 at census; `prediction_cache`
dropped). Empty + wired ≠ DEAD, but each is a promised loop that has never
closed. One-line verdicts:

| Table | Verdict |
|---|---|
| `alignment_score_history` | CLOSE LOOP — writer wired (`analytics alignment current`); routines never call it |
| `annotations` | CULL CANDIDATE — chart-annotation feature never shipped a writer path users reach |
| `broker_connections` | CULL CANDIDATE — broker-import scaffold, no provider ever wired |
| `capital_flows` | CULL CANDIDATE — F59 scaffold, provider never wired (or wire a provider with a named consumer) |
| `chart_state` | CULL CANDIDATE — saved-chart-state feature never shipped |
| `debate_scores` | CULL NEXT — debate mechanism formally retired per EPISTEMICS; transcripts (debates/debate_rounds) retained, scores table never used |
| `dividends` | KEEP — working CLI ledger; portfolio currently holds no dividend payers |
| `gex_snapshots` | BLOCKED — depends on options_chain_snapshots ingest that has never run; wire or cull both together |
| `group_members`, `groups` | KEEP — operator config feature, zero-cost until used |
| `mobile_timeframe_scores` | KEEP — populated only when the mobile API serves; rebuildable L2 |
| `news_source_accuracy` (+ `_events`) | CLOSE LOOP — writer wired into refresh; needs the first scored news-derived prediction to populate |
| `options_chain_snapshots` | BLOCKED — Yahoo options ingest never wired into refresh; wire or cull with gex_snapshots |
| `portfolio_allocations` | KEEP — allocation-% mode (alternative to transactions); mode unused but supported |
| `predictions_history` | CLOSE LOOP (priority) — L1 series with live readers (narrative-divergence 24h deltas silently degrade without it); needs a writer in the predictions refresh path |
| `recommendation_outcomes` | CLOSE LOOP — scoring side of decision-card recommendations (the R4 shadow book deliberately persists nothing: `research shadowbook` computes shadow-vs-actual-vs-hold on demand from the ledger + prices + transactions) |
| `regime_overrides` | KEEP — operator override escape hatch, used on demand |
| `research_questions` | CULL CANDIDATE — backlog table never adopted by any routine |
| `risk_factor_mappings` | CULL CANDIDATE — curated mappings never seeded, no consumer |
| `run_health` | CLOSE LOOP (priority) — the EPISTEMICS spine; report runs must write it or the instrumentation story is fiction |

## Backups

The live DB is irreplaceable personal financial state on one laptop. Back it
up. Archives must live OUTSIDE the repo (default `~/pftui-archives/`) and
are never committed.

- **What**: the active SQLite DB (`pftui system db-info` shows the path),
  plus `config.toml` if you want API keys back after a reinstall.
- **How**: `pftui system archive-db` — atomic `VACUUM INTO` copy, prints
  path + size. `--out PATH` to target an external volume;
  `--table X` exports a single table as JSON instead.
- **Cadence**: weekly, plus before any schema-touching upgrade. The R3 cull
  migration takes its own per-table JSON archives automatically, but those
  are not a substitute for a full backup.

Suggested launchd job (documented, NOT installed — operator's call). Save as
`~/Library/LaunchAgents/com.pftui.backup.plist`, then
`launchctl load ~/Library/LaunchAgents/com.pftui.backup.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.pftui.backup</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/local/bin/pftui</string>
    <string>system</string>
    <string>archive-db</string>
  </array>
  <key>StartCalendarInterval</key>
  <dict>
    <key>Weekday</key><integer>1</integer>
    <key>Hour</key><integer>9</integer>
    <key>Minute</key><integer>0</integer>
  </dict>
  <key>StandardOutPath</key><string>/tmp/pftui-backup.log</string>
  <key>StandardErrorPath</key><string>/tmp/pftui-backup.log</string>
</dict>
</plist>
```

Prune old archives by hand; `archive-db` never deletes anything.

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

## Report integrity (R5) — the pipeline cannot mask

The operator's only interaction with the system is the private PDF. A
misconfigured report pipeline could mask key details, obscure issues, or lie
about missing data — so the assembler (`src/report/build/daily.rs`) carries
four structural guarantees:

### 1. Slot-conformance availability (cannot fall behind)

Every data-bearing field on the report build context is a "slot".
`data_availability()` emits one row per slot, and the
`every_build_context_slot_is_tracked` test parses the struct definition out
of the source and fails when a new field ships without a matching
availability row (metadata fields are declared in
`BUILD_CONTEXT_META_FIELDS`). The schema-conformance pattern, applied to the
report: a new loader without availability tracking is a red test. Do NOT
weaken the test — add the `vec_slot!`/`opt_slot!` row.

### 2. Loader-error honesty (four slot states)

A loader failure must never abort the build (resilience) and must never be
silent (honesty). Every slot resolves to one of:

| status | meaning |
|---|---|
| `populated` | data loaded |
| `no_data` | query succeeded, genuinely nothing there |
| `upstream_not_run` | rows exist for EARLIER dates but none for the report date — the writing phase (parallels runner, investor panel, synthesis writer, decision architect…) did not run today |
| `loader_error` | the query/computation FAILED — carries the error string |

Loaders record `SlotIssue` into the context's `slot_issues` map;
`data_availability()` classifies from it. `loader_error` and `no_data` must
never render identically.

### 3. The integrity footer (private report, unconditional)

`assemble_private*` appends a final small-print block after the last
section, opened by `<!-- integrity-footer: do not remove -->`:

```
Report integrity: N/M slots populated. No data: […]. Upstream not run: […].
LOADER ERRORS: slot: error text (bold). Sections: N rendered, M
auto-suppressed (with each empty-state reason). Stale inputs: […].
```

When everything is populated it collapses to one quiet line. The composition
step edits ABOVE the marker and never removes the block. The full per-slot
table with reasons is the operator's audit surface:
`pftui report build daily --mode private --dry-run --json`
(`data_availability[].status/reason`, `section_outcomes[]`,
`staleness_warnings[]`).

### 4. Staleness annotations + suppression accounting

At build time the assembler compares inputs against their freshness
expectations (analyst views: the 6h skill gate; prices: last fetch vs report
date; economic/sentiment series: their registered `series_registry` SLAs)
and injects a `> ⚠ …` blockquote under the affected section headings —
annotate, never suppress. Section renderers' empty states return
`sections::suppressed(reason)` instead of a bare empty string, so every
auto-suppressed section carries an explicit, machine-readable reason
(enforced by `every_section_empty_state_carries_a_suppression_reason`).
