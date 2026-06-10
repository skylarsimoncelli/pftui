# EPISTEMICS.md — the self-checking architecture

> Added 2026-06-10 after a full-system audit found the epistemic layer was
> "scaffolding without load": 14/14 agent voices endorsed the operator's prior
> stance in a single run, every meta-learning table was empty, scenario
> probabilities moved 4pp in hours via uncoordinated writers, and the
> continuity SQL had been silently broken for weeks behind graceful fallbacks.
>
> Organizing principle: **every epistemic claim the system makes must
> eventually collide with something that can prove it wrong — mechanically,
> not rhetorically.** Correction lives in write-paths, scoreboards, and diffs
> (binding, measurable), never only in prompt prose (advisory, ignorable).

## Subsystem map

| Subsystem | Mechanism | Where |
|---|---|---|
| Independence layer | blind analyst, anti-thesis rival, belief quarantine, panel isolation | `agents/report-prompts/phase1b-blind-analyst.md`, `phase2d-antithesis.md`, `phase1-timeframe-analyst.md`, `phase2b-panel-persona.md` |
| Binding learning loop | falsification rules, mechanical auto-scoring, calibration confidence clamps | `journal prediction add --falsify`, `journal prediction auto-score`, `calibration_matrix` |
| Probability discipline | scenario ledger (evidence, delta caps, conflict guard), base rates | `journal scenario update --evidence/--hard-print/--override-conflict`, `scenario set-base-rate` |
| Memory consolidation | novelty scoring, standing rules, thesis review dates, stale-view detection | `daily_notes.novelty_score`, `analytics lessons rules`, `analytics thesis review-due`, `analytics views stale` |
| Data integrity | economic-data quarantine, BTC-series divergence doctor check, analyst spot-check audit | `economic_data.quarantined`, `system doctor`, report-skill Step 5.5 |
| Instrumentation | run_health table, epistemics command family, report section | `analytics epistemics record/show/history/rivalry`, `private_epistemic_health` |

## 1. Independence layer (anti-sycophancy)

The failure this fixes is architectural, not behavioral: the operator's journal
was injected into every prompt as evidence, with asymmetric friction
(contradicting it required explanation; agreeing was free). Models under that
constraint agree. The fix is **information design**:

- **Blind analyst** (`--analyst blind`): a fifth Phase-1 agent receiving ONLY
  the raw data bundles — no operator journal, no focus, no thesis, no lesson
  book. It is the control group. `blind_divergence` (mean |house − blind|
  conviction across held assets) is the run's sycophancy measurement,
  self-derived by `analytics epistemics record`.
- **Anti-thesis rival** (`--analyst antithesis`, author `analyst-antithesis`):
  maintains the strongest *coherent opposite worldview* with its own web
  research, denied the thesis table and journal. It files falsifiable
  predictions under its own ledger; `analytics epistemics rivalry` compares
  its scored hit rate against the house layers. A rival with a track record,
  not a debate performance.
- **Measurement vs voting layers**: `blind` and `antithesis` are accepted
  writers but are excluded from every convergence/matrix/trajectory
  aggregation (`layer_class: measurement`). Exclusion happens at the
  aggregation/loader layer; `classify_convergence` stays the single source of
  truth.
- **Belief quarantine**: operator journal entries are injected as
  "BELIEF INPUT — NOT MARKET EVIDENCE"; analysts must state agree/disagree per
  relevant belief *symmetrically* and return a required "Where the operator is
  most likely wrong" section (aggregated by the synthesis writer into a
  `[synthesis-operator-wrong]` note). Step-11 interview answers are tagged at
  capture: `[operator-belief]` / `[operator-intent]` / `[operator-fact]`.
- **Panel isolation**: personas no longer receive the operator journal, run as
  a rotating 4-of-8 subset, and their confidence dispersion is measured —
  stddev < 4.0 flags "persona washing".
- **Retired**: the per-asset steelman pairs and debate moderator (they shared
  house priors and produced restatements of the adversary's counter-case —
  diversity of label, not of thought).

## 2. Binding learning loop

- **Falsification at write time**: `journal prediction add --falsify
  "<SYM> close below 50000 by 2026-09-30"` parses deterministically into
  `prediction_falsification_rules`. The rule encodes the claim's SUCCESS
  condition. Unparseable or absent rule → confidence capped at 0.3
  ("unfalsifiable prediction"). The `data/analytics predictions add` aliases
  route through the same discipline.
- **Mechanical scoring**: `journal prediction auto-score` evaluates eligible
  rules against `price_history` closes — no LLM judge, no grading-your-own-
  homework. Runs in the tail of `data refresh` (this machine has no daemon;
  refresh is the recurring surface).
- **Calibration clamps**: at write time, stated confidence is clamped to the
  matrix's trailing hit rate + 0.15 for that (layer, topic, conviction band)
  when n ≥ 8. `--override-confidence-cap --cap-rationale` is the logged escape
  hatch. The model is not trusted to remember its own overconfidence; the
  write-path remembers for it.
- **Standing rules**: the 196-row lesson sediment is consolidated into ~10
  operational rules (`standing_rules` table, `analytics lessons rules`).
  Prompt injection = 10 standing rules + the 5 most recent unconsolidated
  lessons. `rules cite <id>` counts violations.

## 3. Probability discipline

- `scenario update --probability` requires `--evidence`; every move is
  ledgered in `scenario_updates` with proposer and old→new values.
- Cumulative |Δ| per scenario per day is capped at 5pp; exceeding it requires
  `--hard-print "<the data print>"`. Same-day updates by different proposers
  require `--override-conflict`. Analysts propose; synthesis applies.
- `scenario set-base-rate` records a reference-class base rate; `scenario
  list` shows the deviation. A scenario priced far from its base rate without
  justification is an exaggeration flag (the audit's example: "Risk-On Rally
  2%" against a ~70% historical base rate for up-years).
- Parallels output (`pftui-parallels-run`) now carries `sample_era` (per-year
  match counts) and `recency_weighted_pct` (4-year half-life weighting,
  matching documented cycle dampening). Report R/R probabilities must come
  from these empirical distributions where a set matches, or be labeled
  "illustrative, uncalibrated".

## 4. Memory consolidation

- **Novelty scoring**: every `journal notes add` computes trigram-Jaccard
  similarity vs the author's last 20 notes; ≥85% similarity prints a
  "repetitive — promote to thesis or stop re-deriving" notice.
  `journal notes repetition` clusters an author's repeats.
- **Stable vs stale**: `analytics views stale` flags views older than 21d
  whose asset moved >10% since the view — evidence moved, conviction didn't.
  `analytics thesis set-review/review-due` puts review dates on thesis rows.
- **Loud degradation**: every empty-state fallback in the report skill appends
  to a warnings log surfaced in the run summary and counted in `run_health`.
  Silent graceful fallbacks are how the broken continuity SQL went unnoticed.

## 5. Data integrity

- `economic_data` rows failing per-indicator plausible ranges are stored
  `quarantined=1`, skipped by all readers, and rendered as
  "unavailable (failed sanity check)" — garbage never reaches a published
  newsletter silently.
- `system doctor` checks BTC vs BTC-USD latest-close divergence (>2% fails).
- The accuracy auditor spot-checks 5 analyst-sourced numeric claims per run
  (the blanket "analyst credibility" pass is retired); failures feed the
  layer's audit pass-rate.

## 6. Instrumentation — run_health

One row per report run (`analytics epistemics record`, upsert merges
field-wise):

| Metric | Threshold flag |
|---|---|
| `agreement_rate` | > 0.85 → ⚠ echo risk |
| `blind_divergence` | > 2.0 → ⚠ house view far from raw-data read |
| `panel_dispersion` | < 4.0 → ⚠ persona washing |
| `fallback_warnings` | > 0 → listed in the run summary |
| `audit_pass_rate` | informational, trended |
| `scenario_delta_total` | informational (self-derived from the ledger) |

`analytics epistemics show/history` render flags and trends; the
`private_epistemic_health` report section (last in the private plan) puts the
table in front of the operator; `analytics epistemics rivalry` is the
house-vs-antithesis scoreboard.

## Operating notes

- **No daemon.** pftui is driven entirely by Claude Code invocations. All
  recurring mechanisms fire via `data refresh` and report-skill steps. True
  background cadence (e.g. scoring a prediction the day its window closes)
  would need launchd or a scheduled cloud agent — a deliberate operator
  decision.
- **Cadence**: the full multi-agent pipeline is sized for weekly runs; daily
  check-ins use the report skill's light mode (no fan-out, level-percentile
  framing on tape deltas).
- **The friction is the product.** The blind analyst will disagree when the
  operator is right; clamps will frustrate genuine conviction; base-rate
  anchors will make scenarios sluggish when the world really is changing.
  These systems optimize for being checkable, not for feeling coherent.
