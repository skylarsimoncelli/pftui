# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P2 - Coverage And Agent Consumption


### `pftui report build daily` — umbrella tracker (do not pick directly)
**Source:** Skylar (May 28). Depends on both `pftui report` scaffold and the chart-helper-port items above.
**Why:** Once chart rendering is native, the next layer up is the report ASSEMBLY — pulling data, ordering sections, inlining charts, and writing the markdown that feeds the PDF renderer. Today that work lives in the Claude `/pftui-report` skill orchestration plus the ad-hoc Python build script generated per run. Making it a native `pftui report build daily` command means: (1) anyone (not just Claude) can build a daily report from a populated DB; (2) the assembly logic gets `cargo test` coverage; (3) the Claude skill becomes much thinner — it spawns analysts, then calls `pftui report build daily`, then PRs the output. (4) Removes the Python build script entirely from the steady-state pipeline.
**Implementation plan:** All section TODOs and the assembler are landed. Remaining work is the skill migration below.
**Effort:** Complete except for the skill migration item.

### Migrate `/pftui-report` Claude skill to use native `pftui report` commands
**Source:** Skylar (May 28). Depends on `pftui report build daily` (above) being landed.
**Why:** Now that `pftui report build daily` exists end-to-end, the Claude skill at `~/.claude/skills/pftui-report.md` can be substantially simplified: no ad-hoc Python build script per run, no per-step data-gathering bash blocks that prepare chart inputs. The skill's responsibilities shrink to: Step 0 health collection + blocker fixes, Step 1 data refresh, Step 3 spawning the four analyst subagents, then calling `pftui report build daily --mode <m>`, then the privacy audit / PDF render / website registry / PR steps.
**Scope:** (1) Rewrite the relevant sections of `~/.claude/skills/pftui-report.md` (Step 2 CLI bundle, Step 2b deep bundle, Step 2c thesis/lessons fetch, Step 4 synthesis, Step 5a public markdown, Step 5b private markdown) to call `pftui report build daily` instead of doing data collection + assembly in skill bash + Python. The bundles can still be staged for the analysts (they need them as input), but the synthesis-and-write step becomes a single CLI call. (2) Decommission `~/pftui-operator/charts.py` once all charts are ported and used by zero remaining code paths — leave the file but mark it deprecated in a header comment and remove the skill's `sys.path.insert` line. (3) Update the skill's failure-modes section: `pftui report build daily` errors should be diagnosed by reading the command's stderr; the skill's responsibility is to surface those errors, not to debug section assembly. (4) Run `/pftui-report` end-to-end at least twice on the new code path before considering this item done; compare the resulting markdown + PDFs against the prior Python-orchestrated outputs and confirm parity. Files: `~/.claude/skills/pftui-report.md` (substantial rewrite), `~/pftui-operator/charts.py` (deprecation header). Tests: not applicable in pftui (skill-side change); verification is the parity comparison.
**Effort:** 4–7 days (mostly skill testing + iteration).

---

## P3 - Long Term

### Options flow + GEX (gamma exposure) ingestion
**Source:** Claude DB enrichment session (June 1). The single most-impactful missing data input identified across the substrate.
**Why:** 27 lessons in the `tight_threshold_close_miss` cluster and 14+ predictions in `options-gamma-pinning` fragment territory all share a root cause that's invisible to the current ingest: options gamma concentration at round-number strikes mechanically pins prices. SPY $700, BTC $75k, gold $5000 — all repeated threshold misses where the prediction direction was right but the close pinned to the level. Without options-flow data, the `options-gamma-pinning` and `tight-threshold-coin-flip` fragments are heuristics applied retrospectively. With it, they become computed: "current SPY GEX puts gamma flip at 745; predictions through 745 need to clear by 1.5xATR + gamma-zone width." This is the single new ingest that would directly upgrade the most-recurring miss pattern.
**Scope:** (1) New data source `pftui data options [--symbol SPY] [--strike-window 10] [--json]` pulling from a free or low-cost options-flow provider (research candidates: Polygon options snapshot endpoint, CBOE OI data, or unofficial gex.app scraping if licensing allows). (2) New tables: `options_chain_snapshots (symbol, strike, expiry, dte, oi_calls, oi_puts, vol_calls, vol_puts, iv_atm, fetched_at)` and `gex_snapshots (symbol, gex_flip_strike, total_gamma_call, total_gamma_put, max_pain, fetched_at)`. (3) Refresh integration: `data refresh` pulls daily snapshots for SPY, QQQ, BTC (via deribit), GLD, SLV, and held single-name positions if any. (4) `pftui analytics gex --symbol <s> [--json]` returns the current snapshot + the "gamma neutral" zone. (5) Pre-flight integration: when a prediction targets a level within a known gamma zone, surface a warning. (6) Daily report: per-asset section adds a one-line "GEX flip at $X, max pain $Y" inline. (7) Backfill from historical OI data if the provider supports it (typically last 90 days). Files: `src/data/options.rs` (new), `src/db/options_chain_snapshots.rs` + `src/db/gex_snapshots.rs` (new), `src/commands/data.rs`, `src/commands/analytics.rs`, `src/cli.rs`, the report skill. Tests: data fetch against mocked provider; gex computation against fixture chain; preflight integration.
**Effort:** 3–4 weeks (most of which is selecting + integrating a data source).

### F59: Capital Flow Tracking
**Source:** Competitive research (NOFX institutional flow data).
**Why:** Institutional fund flows, ETF creation/redemption, and open interest changes reveal positioning that price alone doesn't show.
**Scope:** New `data flows` source pulling ETF flow data (ETF.com or similar), institutional 13F filings, and crypto exchange flow data. New table `capital_flows`. Integration into agent routines.
**Effort:** 3–4 weeks.

### [Claude-WIP 2026-06-02j — DO NOT PICK] Thesis dependency graph — LLM-assisted extraction backfill
**Source:** Follow-up to the 2026-06-02 cross-asset thesis dependency graph PR. That PR landed the `thesis_dependencies` table, `pftui analytics thesis-chains list|show|validate|add`, the price-threshold validator, the `journal prediction preflight` integration, and a `report::sections::thesis_chains_macro::render_thesis_chains_block` renderer.
**Why:** Chains are currently authored by hand via `thesis-chains add`. The fastest way to seed 30-60 high-quality chains is a one-shot Opus extraction pass over `thesis.content` + `prediction_lessons.why_wrong` + last-90d `agent_messages`.
**Scope:** (1) Add an Opus subagent or `pftui agent` command that reads the three sources, emits JSONL `{antecedent_text, relation, consequent_text, conviction, source_lesson_ids, source_thesis_sections, evidence_count}` triples, and calls `analytics thesis-chains add` for each row. (2) Enrich the validator to handle additional predicate shapes (range thresholds, derived metrics like real_yield, DXY-spread). (3) Auto-wire the `thesis_chains_macro::render_thesis_chains_block` output into the daily-report Macro section assembler once the assembler exposes a chain-loading hook. (4) Tests: extraction produces valid triples; auto-wire respects the public-mode privacy guard (chains do not leak portfolio-specific framing).
**Effort:** 1 week (mostly subagent prompt engineering + Opus call budget).

---
