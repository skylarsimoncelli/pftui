# TUI Glance-Value Program — surfacing inventory and cockpit design

> Scoping document (2026-06-11). Operator directive: "I very much want to keep
> this and improve it at some point. I do genuinely love it, and if it was
> more valuable to glance at then I would glance at it more."
>
> Scope: **pure surfacing.** Everything below reads existing tables and
> existing deterministic engines. NO new analytics, NO new tables, NO new
> classifications (`classify_convergence` and friends stay the single source
> of truth). Implementation briefs live in TODO.md under
> `### TUI Glance-Value Program`.

## 1. What the TUI shows today

Eight tabs (`ViewMode`, `src/app.rs:34-43`; keys `1`-`8`):

| Tab | Key | File | Data rendered |
|---|---|---|---|
| Positions | `1` | `views/positions.rs` | Holdings table: price, day Δ, value, gain, allocation, RSI sparkline, 52W range, category dividers; privacy table variant |
| Transactions | `2` | `views/transactions.rs` | Buy/sell ledger (full mode) |
| Markets | `3` | `views/markets.rs` | Index/commodity/crypto/forex quotes, mini sparklines, COT signal column, 7d momentum, prediction-market panel |
| Economy | `4` | `views/economy.rs` | Treasury yields + curve chart, BLS indicators, macro table, sentiment (F&G) panel, calendar, predictions panel |
| Watchlist | `5` | `views/watchlist.rs` | Tracked symbols, target proximity, alerts, groups |
| Analytics | `6` | `views/analytics.rs` | Risk metrics (vol/Sharpe/MaxDD/VaR/HHI), concentration, **what-if stress presets** (Oil $100, BTC $40k…), projection, regime monitor, impact panel |
| News | `7` | `views/news.rs` | RSS entries, category color, context panel |
| Journal | `8` | `views/journal.rs` | Journal entries table + detail panel |

Widgets: header (tabs, portfolio value, privacy indicator), status bar,
sidebar, portfolio sparkline/stats, allocation bars, top movers, regime bar +
regime assets, market context, price charts (braille, SMA/BB, crosshair),
skeleton loaders. Popups: asset detail (rich: price, key levels, technicals,
portfolio context, thesis, BTC intelligence, COT, COMEX), position detail,
alerts, search chart, scan builder, command palette, context menu.

Data path: `App::load_data()` (app.rs ~1146) reads cached tables through
`BackendConnection` at init and after the background-refresh thread signals
completion via mpsc (`background_refresh_complete_rx`). No blocking I/O in
the event loop — that is the pattern every new widget must follow.

## 2. What the report/CLI surface that the TUI does NOT

Verified by grep over `src/tui/`, `src/app.rs`, `src/ui.rs` — zero hits for
every row below (the only "scenario" hits are the Analytics-tab what-if
presets; the only "cyber" hit is the Neon theme comment).

| Capability | Where the data lives | Existing reader (CLI / report) | TUI |
|---|---|---|---|
| Four-layer analyst views + convergence classification | `analyst_views` (L3), `db/analyst_views.rs` (`get_portfolio_view_matrix_backend`, `classify_convergence`, `ConvergenceReport`) | `analytics views convergence[-all]`, report per-asset cards | **absent** |
| PROBATION state / misalignment streaks | `forecast_misalignments` (L3), `db/forecast_misalignments.rs` (`active_misalignments`, `active_probation_map`) | `research misalignments`, convergence caps, report epistemic block | **absent** |
| run_health / epistemic health (echo flags, hit rates, dispersion) | `run_health` (L3), `db/run_health.rs` (`get_latest_run_health`, `threshold_flags`, `compute_forecast_hit_rate`) | `analytics epistemics show/history/rivalry`, `private_epistemic_health` section | **absent** |
| Recommendation ledger + scoreboard + window-quality | `recommendations` (L3), `db/recommendations.rs` (`list`, `rolling_hit_rate`, `accuracy_summary`, scoreboard in `commands/`) | `analytics recommendations scoreboard/list` | **absent** |
| Shadow book (SHADOW/ACTUAL/HOLD 3-NAV) | computed on demand from ledger+prices+transactions, `research/shadow_book.rs::compute` | `research shadowbook`, epistemics summary line | **absent** |
| Signal expectancy (event studies, lift) | `signal_expectancy` (L2), `db/signal_expectancy.rs::latest_rows`; registry `research/registry.rs` | `research expectancy/events/backtest`, report "Signal expectancy" line | **absent** |
| Market-structure verdicts (HH/LL, BOS, daily+weekly) | pure fn over `price_history`: `analytics/market_structure.rs::analyze` → `StructureRead.verdict` | `analytics technicals structure` | **absent** |
| Cycle-engine verdicts (timing bands, translation, FLD/VTL) | pure fn: `analytics/cycle_engine.rs::analyze` → `DegreeStatus` (band_position, bars_to_band_*); `cycle_clock.rs` (BTC halving / gold ~7yr) | `analytics cycles analyze/ledger/clock` | **absent** |
| Cyber Dots state (QB, dots, Pi proximity) | pure fn: `analytics/cyber/mod.rs::analyze` → `CyberSnapshot` (verdict, dots, pi_cycle, signals) | `analytics technicals cyber` | **absent** |
| Series-registry freshness | `series_registry` (L1 meta), `db/series_registry.rs::status_all` | `data series status`, `system doctor` 2×-SLA check | **absent** |
| Standing rules | `standing_rules` (L4), `db/standing_rules.rs::list_rules` | `analytics lessons rules` | **absent** |
| Scenario ledger + base-rate deviations | `scenarios`/`scenario_updates` (L3), `db/scenarios.rs` (`list_scenarios`, `compute_normalized_set`) | `journal scenario list`, report scenarios section | **absent** (Analytics-tab "scenarios" are unrelated what-if presets) |
| Decision cards | `agent_messages` rows `category='decision-card'`, `db/agent_messages.rs::list_messages_backend` | `private_decisions_pending` report section | **absent** |
| Parallels distributions | external `pftui-parallels-run` JSON output, not in the DB | report R/R probabilities | **absent — and stays out of scope** (no DB home; surfacing would require new storage, violating the pure-surfacing rule; revisit if/when parallels output gets an L2 cache) |

## 3. Glance-value ranking

Operator profile: HTF physical-metals + BTC accumulator who checks in
occasionally. The decisions he actually makes are slow — add / wait / trim —
so glance value = **deterministic verdict density + "does anything need my
attention" + "is the system's advice working"**, not prose. Ranked by
glance-value per screen-inch:

1. **Per-held-asset verdict board** (structure D/W + cycle band + Cyber QB/Pi
   + convergence glyph + probation flag, one row per asset). This is the
   single highest-density screen the system can draw: five orthogonal
   deterministic reads per asset, currently reachable only via five separate
   CLI invocations per asset. It is the screen that makes daily opening
   worth it.
2. **Attention strip: probation + stale feeds + epistemic flags.** Misaligned
   forecast streaks (PROBATION), series past 2× SLA, and run_health threshold
   flags are precisely the "something is wrong, look closer" signals — the
   reason to glance at all. Tiny screen cost (3-4 lines).
3. **Shadow-vs-actual-vs-hold + recommendation window-quality.** "Is the
   system's advice making money" in one sparkline + one number per symbol.
   Highest trust-building value; the operator funds his attention with this.
4. **Scenario board with base-rate deviation.** The probability ledger is the
   system's worldview in 6 lines; deviation-vs-base-rate flags exaggeration
   at a glance.
5. **Signal expectancy in the asset detail popup.** Contextual, not ambient:
   when a verdict glyph changes, Enter on the row should show *measured*
   lift for the signals that just fired — the popup already has the
   Technicals/Key-Levels skeleton for it.

Confirmed prior: a cockpit of deterministic verdict glyphs beats porting any
report prose. Prose ages in hours; verdicts are recomputed from cached
`price_history` on every refresh and are exactly what the four external CLI
engines already emit.

## 4. The cockpit design

New tab: **`[9] Intel`** (`ViewMode::Intel`; key `9` is globally unbound —
verified against `handle_key`). Layout, 80×24 minimum:

```
┌ pftui ─ [1]Pos [2]Tx [3]Mkt [4]Eco [5]Watch [6]An [7]N [8]J [9]Intel ──────┐
│ ⚠ ATTENTION  PROBATION: analyst-low/GC=F (4-streak)   STALE: cot_cache 17d │
│ EPISTEMICS 06-10  agree .72  blind 1.4  disp 5.1  fwd-hit 58% (n=31)  ✓ ok │
├─ Verdict Board ────────────────────────────────────────────────────────────┤
│ ASSET   STRUCT D/W   CYCLE BAND        CYBER          CONV          FLAGS  │
│ BTC     ▲HH·HL ▲HH   in-band 62%  18wk QB:bull ●3 Pi·far  ++3.2 conv-bull   │
│ GC=F    ▲HH·HL ─rng  pre-band -22d     QB:bull ●1 ·       +1.8 cv-neutral ⚠P│
│ SI=F    ─rng   ▲HH   in-band 41%       QB:neut ●0 ·       +0.6 divergent    │
│ GLD     ▲HH·HL ▲HH   (tracks GC=F)     QB:bull ●2 ·       +2.1 conv-bull    │
├─ Ledger ──────────────────────────┬─ Scenarios ───────────────────────────┤
│ SHADOW 104.2  ACTUAL 102.9  HOLD  │ Risk-On Rally   24%  base 70%  ▼-46pp │
│ 101.7   (90d, indexed)  ⠉⠒⠤⠼⠶⠦   │ Gold Consolid.  38%  base 31%  ▲ +7pp │
│ ADD−WAIT 90d: GC=F -1.2%  BTC +3.4│ Liquidity Crack 11%  base  9%  · +2pp │
└─ j/k select · Enter asset detail · ? help ────────────────────────────────┘
```

Glyph vocabulary (theme-aware via `app.theme` — gain/loss/warning slots, all
11 themes): `▲`/`▼`/`─` structure class (uptrend/downtrend/range), `●N`
Cyber strength dots, `⚠P` probation, convergence shows signed avg conviction
+ abbreviated `classify_convergence` summary. Everything renders from a
single `IntelSnapshot` computed off cached tables in the existing
background-refresh thread — zero network, zero event-loop blocking. Privacy
mode: the board is value-free by construction; the shadow NAV strip renders
indexed to 100 (never dollars), so it is privacy-safe in both modes.

Empty states are loud, not blank (EPISTEMICS doctrine): `run_health` empty →
"no run recorded — epistemics never written on this machine"; no analyst
views in window → "insufficient-views"; engines need ≥ N bars → "short
series".

## 5. G8 — Synthesis Browser ("the work behind the report")

> Extension (2026-06-11, same session as the program). Operator directive:
> "the TUI should have options to surface some more of the underlying
> synthesis that goes into making the reports — all of the work in the
> background done by the agents is lost and never surfaced." Briefs:
> TODO.md G8.1-G8.3. Pure surfacing — the substrate below already exists,
> is written every report run, and is read today only by `jq` and the
> report assembler.

### 5.1 The substrate inventory (verified by grep over the writers)

Every report run (`/pftui-report`, phases in `agents/report-prompts/`) and
every analyst routine (`agents/routines/`) writes its reasoning into five
L3 stores. None of it is reachable from the TUI; the only human surface is
the *output* PDF, which compresses ~50 notes + ~20 messages into prose.

| Store | What the writers put there | Writers (grep-verified) | Existing readers | TUI |
|---|---|---|---|---|
| `daily_notes` (L3) — `id, date, section, content, author, created_at, novelty_score` (`src/db/daily_notes.rs::DailyNote`) | Layer analyses (thesis / evidence / disconfirming / what-would-change-my-mind, 3-8 per layer per run), `[synthesis-<SYM>]` per-asset cards, `[synthesis-economy]`, `[synthesis-macro-outlook]`, `[synthesis-closing]`, `[synthesis-operator-wrong]`, `[synthesis-deep-dive]` essays, `[synthesis-external-ta]`, `[antithesis]`, `[debate-roundup]`, `[operator-interview-*]`, evening/morning post-mortems, `--stamp`ed market-snapshot first lines | phase1 (`--section <layer> --author analyst-{LAYER}`), phase2c/2d, phase3, phase3b, phase6, step11, all six analyst routines | `journal notes list/search [--author --section --date]`, report assembler | **absent** |
| `agent_messages` (L3) — `from_agent, to_agent, package_id/title, priority, content, category, layer, acknowledged` (`src/db/agent_messages.rs`) | Cross-layer signals (`--category signal --layer <layer>`), `[operator-wrong {LAYER}]` per layer, `panel-<persona>` JSON responses (renderer keys on the `panel-` sender prefix), decision cards (`category='decision-card'`), steelman bull/bear (phase5), R/R notes, `operator-intent` | phase1, phase2b (via orchestrator), phase3, phase4, phase5, step11 | `agent message list [--from --to --layer --since --unacked]`, `private_decisions_pending` report section | **absent** |
| `analyst_views` + `analyst_view_history` (L3) — `reasoning_summary, key_evidence, blind_spots, allocation_bias` per analyst×asset (`src/db/analyst_views.rs::AnalystView`) | The structured WHY behind every conviction number the convergence glyph (G2) compresses to `++3.2` | 4 timeframe routines, every run, every held asset | `analytics views matrix/list`, convergence paths | **absent** (G2 surfaces only the classification) |
| `adversary_synthesis_views` (L3) — `counter_case_summary, counter_case_evidence_points, falsification_triggers, fragility_score 1-5` (`src/db/adversary_synthesis_views.rs`) | The argument AGAINST the four-layer convergence, same data; fragility ≥ 3 gates synthesis | phase2a / `agents/routines/adversary-analyst.md` | `analytics adversary-synthesis list/fragility`, `adversary_view` report section | **absent** |
| `forecast_misalignments` (L3) — streak dossiers: `layer, asset, streak_len, call, cum_realized_against_pct, status` | The wrong-sign-streak evidence behind every `⚠P` glyph on the verdict board | refresh tail (`detect_and_update`) | `research misalignments`, convergence caps | G3 shows the FLAG; the dossier behind it is **absent** |

The asymmetry is the point: the system writes ~50-80 reasoning artifacts per
report run and the operator can read exactly none of them without `sqlite3`
or `jq`. G8 is the read path.

### 5.2 The tag taxonomy (read-time, presentation-only)

Notes self-describe through two orthogonal axes that already exist —
**author** (the canonical `analyst-*` registry in CLAUDE.md) and a leading
**`[bracket-tag]`** in `content` written by the report-prompt templates
(grep-verified vocabulary above). The browser derives its phase tree from a
pure function `parse_note_tag(&str) -> Option<NoteTag>` (strip the leading
`[...]`, match the known prefixes, extract `<SYM>` from `synthesis-<SYM>`)
plus the author registry. This is presentation classification, NOT a new
analytical classification — no table, no column, unit-tested, and unknown
tags land in a visible "other notes" node, never dropped (tree counts must
sum to the window's row count — loud-completeness rule).

### 5.3 The browser: three panes, email-client shape

A full-screen drill-in view (`ViewMode::Synthesis`), reached from the Intel
tab — NOT a tenth numbered tab (digit budget is spent; `0` is already
Analytics-scoped). Header renders it as `Intel ▸ Synthesis`. 80×24 minimum:

```
┌ pftui ─ Intel ▸ Synthesis ── filter: author:all · asset:GC=F · 14d window ─┐
│ Runs ────────┬ Run 06-10 — phases ─────────┬ [synthesis-GC=F] ─────────────┤
│▸06-10 47n 18m│   1 Layers                  │ analyst: synthesis-writer     │
│ 06-09 31n 12m│     analyst-low    4n 3m    │ 06-10 14:02 · novelty 0.62    │
│ 06-08 29n  9m│    ▸analyst-medium 5n 4m    │───────────────────────────────│
│ 06-07  8n  2m│     analyst-high   3n 3m    │ [synthesis-GC=F]              │
│ 06-05 44n 16m│     analyst-macro  6n 5m    │ Direction: bear-to-neutral.   │
│ 06-04 41n 15m│   2 Views (4×GC=F)          │ Daily structure repaired the  │
│ 06-03 38n 14m│   3 Adversary  frag 4 ⚠     │ May BOS but weekly remains... │
│ ...          │   4 Panel (4 personas)      │                               │
│              │   5 External TA             │ Evidence: weekly LL intact;   │
│              │  ▸6 Synthesis cards         │ COT commercials -12k; FLD     │
│              │     [synthesis-GC=F]      ◀ │ down-cross target 3,180...    │
│              │     [synthesis-economy]     │                               │
│              │   7 Deep dive               │ Disconfirming: reclaim of...  │
│              │   8 Debate roundup          │                               │
│              │   9 Decisions (2 cards)     │ ▼ 62% · j/k scroll            │
│              │  10 Dossiers (1 active ⚠P)  │                               │
├─ h/l panes · j/k move · Enter open · a author · @ asset · / search · Esc ──┤
```

- **Left — run list.** One row per distinct `daily_notes.date` in the
  loaded window, with note + message counts. The run-date is the natural
  unit: every report-pipeline artifact is keyed to it.
- **Middle — phase tree.** Fixed order mirroring the pipeline (the order
  the work actually happened): Layers → Views → Adversary → Panel →
  External TA → Synthesis cards → Deep dive → Debate → Decisions →
  Dossiers → other notes. Layer nodes expand to per-author notes and their
  `signal`/`[operator-wrong]` messages; Views nodes render
  `AnalystView.reasoning_summary/key_evidence/blind_spots` per layer for
  each asset; Adversary shows `fragility_score` inline (warning color ≥ 3);
  Dossiers lists `forecast_misalignments` rows (active first).
- **Right — preview.** Full text of the selected artifact, scrollable,
  with the metadata line (author, timestamp, section, novelty score) and,
  for messages, category/layer/priority/ack state. Adversary previews
  pretty-print the JSON evidence/trigger arrays as bullets.

Loaders (all existing, no new queries beyond filters already supported):
`db::daily_notes::list_notes_backend` (date/section/author filters) +
`search_notes_backend` for `/`; `db::agent_messages::list_messages_backend`
(client-side date grouping on `created_at` — the fn filters by
from/to/layer/since); `db::analyst_views::get_portfolio_view_matrix_backend`
+ `get_view_history_backend` (history pinned to the selected run-date);
`db::adversary_synthesis_views::list` and
`db::forecast_misalignments::list_all` (both take `&rusqlite::Connection` —
use `backend.sqlite()`, same note as the shadow book in G1).

### 5.4 The critical UX question: report claim → reasoning in ≤ 5 keystrokes

The private PDF says *"GC=F: convergent-bear, adversary fragility 4 —
trimmed conviction."* The operator wants the WHY. Path:

```
9            → Intel tab
j/k          → select GC=F on the verdict board
s            → Synthesis Browser opens PRE-FILTERED: asset=GC=F,
               run=latest, middle pane focused on the [synthesis-GC=F] card
Enter        → full card in the preview pane
j            → next artifact in tree order: the four layer views behind it,
               then the adversary counter-case, then the dossier
```

Four keystrokes to the synthesis card, five to the layer reasoning or the
adversary's counter-case. The pre-filter is the load-bearing design move:
`s` on a selected verdict-board row carries `(symbol, latest run-date)`
into the browser and lands focus on that asset's synthesis card; `s` with
no row context opens unfiltered at the latest run. Every claim in the
report's per-asset card maps to a phase node in the tree, because the tree
IS the pipeline that produced the report.

### 5.5 Pagination + loading contract (642+ notes and growing)

All loads follow the G1 substrate pattern — computed off the event loop,
delivered over mpsc, never blocking a render:

- **Initial window:** the most recent **14 distinct run-dates** (notes +
  messages + views-history + adversary rows for those dates), loaded by the
  background-refresh thread into a `SynthesisIndex` alongside
  `IntelSnapshot`. At current volume (~640 notes total, ~30-50/day on
  report days) a 14-date window is roughly 300-600 rows / ~1 MB — cheap;
  the contract matters for growth, not for today.
- **Older windows:** selecting the `...` sentinel past the window tail
  sends a request on a worker channel (sibling of
  `background_refresh_complete_rx`); the run list shows a skeleton row
  (`widgets/skeleton.rs`) until the next 14-date window arrives. Windows
  accumulate (append, no eviction — bounded by total corpus size, which is
  text).
- **Search:** `/` runs `search_notes_backend` on the worker channel
  (debounced on Enter, not per-keystroke); results render as a flat
  virtual "search" run in the left pane. Esc restores the date view.
- **Full-text residency:** `DailyNote` rows arrive with content (the
  existing loader has no metadata-only mode and adding one is not worth a
  second query shape at this corpus size). Revisit only if a window load
  ever exceeds ~50 ms in the worker — note it in the brief's test budget.

### 5.6 Keybindings (collision-checked against `handle_key`, app.rs:2667+)

`ViewMode::Synthesis` scopes its own keys (precedent: per-view clusters
like Watchlist `a/c/r`), so only globals matter for collisions:

| Key | Action | Collision check |
|---|---|---|
| `s` (Intel view only) | open browser (pre-filtered if a row is selected) | `Char('s')` lowercase is **unbound everywhere** (grep 2026-06-11); `S` is Positions-scoped — untouched |
| `h`/`l` or `Tab`/`Shift-Tab` | pane focus left/right | `h`/`l` are chart-timeframe keys in chart contexts only; inside Synthesis they're view-scoped |
| `j`/`k`, `gg`/`G`, `Ctrl+d/u` | move within pane / scroll preview | standard cluster, view-scoped |
| `Enter` | expand node / focus preview | view-scoped |
| `a` | cycle author filter (all → analyst-low → … → skylar) | `a` is sort/alert in other views; view-scoped here |
| `@` | cycle asset filter (held assets → all) | `@` unbound globally |
| `[` / `]` | previous / next run-date | `[`/`]` exist in other view scopes; view-scoped here |
| `/` | full-text note search | `/` is the global search key — Synthesis intercepts it for note search (same pattern as the search overlay owning `/` while open); help text must say so |
| `Esc` | collapse → back to Intel | standard |
| `1`-`8`, `9` | leave to that tab (pass through) | deliberate: digits always navigate |

Help (`?`) gains a Synthesis section; docs/KEYBINDINGS.md gains the table.

### 5.7 Privacy + theme

Notes and messages are free text that routinely reference position sizes
and dollar amounts (`--stamp` lines are market data — safe; bodies are
not). Free text cannot be reliably scrubbed, so the rule is structural,
mirroring the Transactions tab ("full mode only"): in privacy view
(`is_privacy_view`), the run list and phase tree render normally (counts,
authors, tags — value-free by construction) but the **preview pane**
renders a loud placeholder: `content hidden in privacy view — p to
toggle`. No partial masking — masked-but-guessable is worse than hidden.
Theme: structural panes use existing slots (`text_accent` headers,
`text_secondary` metadata, warning slot for fragility ≥ 3 / `⚠P`); no new
slots, all 11 themes by construction.

## 6. G9 — Asset Technicals Panel (the TA overhaul, surfaced)

> Operator directive: "now we have done an overhaul on the technical
> analysis capabilities, this needs to make its way to the TUI. Selecting
> an asset should show all of the computed technicals, in a polished
> UI/UX." Briefs: TODO.md G9.1-G9.2 (+ G9.3 stretch).

### 6.1 Today vs the computed surface

`asset_detail_popup.rs::build_lines` renders the OLD technicals: SMA
20/50/200 vs price, Bollinger, RSI(14) gauge, MACD — snapshot indicators
that predate the engine overhaul — plus Key Levels, COT, COMEX, BTC
intelligence. The overhaul's actual computed surface (all pure functions
over cached `price_history`, all currently CLI-only):

| Engine | Output (exact structs) | TUI today |
|---|---|---|
| `analytics/market_structure.rs::analyze` (daily AND weekly) | `StructureRead`: `structure` class, `swings` (last 4-6: date, kind HH/HL/LH/LL, price), `last_support_break`/`last_resistance_break` (`BreakEvent`: date, level, swing_date), `ma: MaPosture` (fast/slow values, above flags, slopes, `extension_pct_vs_slow`, `rule13_extension_gate`), `verdict` | **absent** |
| `analytics/cycle_engine.rs::analyze` | `CycleReport.degrees: Vec<DegreeStatus>` per degree: `cycle_age_bars`/`expected_len_bars` + `unit`, `band: BandStats` (`p15_bars`/`p85_bars`, `band_lo/hi`, basis), `band_position` (`pre_band/in_band/over_band`), `bars_to_band_start/_end`, `ledger` (≤8 RT/LT entries) + `rt_string_intact` + `translation_warning`, `fld: FldStatus` (offset, value, price_side, `last_cross` with `target`/`achieved_pct`/`active`), `vtl: VtlStatus` (valid/intact/broken, `break_confirms`), `failed_cycle`, `half_cycle_low`, `possible_inversion` + note, `clarity` (green/amber/red) + issues, `small_n`; plus `btc_clocks`/`gold_clock`, `composite_verdict` | **absent** |
| `analytics/cyber/mod.rs::analyze` (daily; weekly optional) | `CyberSnapshot`: `bands_gaussian` (QB state, `qb_since`, `qb_bars`, transitions), `line` (value, slope, price_above, last_cross), `dots` (up/down strength, SuperTrend dir + stop, VMA/SMA distance %), `pi_cycle` (`top_ratio`/`bottom_ratio` — 1.0 = trigger — + last fires), `mtf_rsi` (RSI6 d/w/m, zone, gating), `breakout`, `signals: Vec<SignalEvent>` (dated, newest first), `verdict` | **absent** |
| `db/signal_expectancy.rs::latest_rows` | per `(signal_id, signal_version, asset, horizon)`: `n_nonoverlap`, `mean_pct` vs `baseline_mean_pct` (`mean_lift`), `hit_rate`/`hit_lift`, `mae_mean`/`mae_worst`, `p_value`/`significant` | **absent** (G6 scopes the popup section) |

### 6.2 Layout A — single sectioned scroll (mocked, not recommended)

Keep one `build_lines` stream, append four engine sections after
"Technicals":

```
│  Structure ─────────────────────────────│   ↑ ~40 lines above
│  D: uptrend  HL 06-02 2,341 · HH 05-28… │
│  W: range    …                          │
│  Cycles ────────────────────────────────│   ← reached after ~60 j-presses
│  18wk: in_band 62% · band 112-131d …    │
```

Honest assessment: the popup already scrolls ~12 sections; the full engine
surface adds **~120-180 lines per asset**. Finding the FLD target means
scrolling past the swing ledger every single time; "polished UI/UX" dies
by scroll. Rejected as the primary layout — but the section renderers
built for Layout B are reusable line-builders, so nothing is wasted if we
ever want a "full scroll" mode.

### 6.3 Layout B — sub-tabbed popup (mocked, **recommended**)

`AssetDetailState` gains `tech_tab: TechTab` + per-tab scroll (popup state
struct already exists — the stateless-widget rule applies to render fns,
and `AssetDetailState` is the established precedent). Five sub-tabs; a
three-line pinned header that never scrolls:

```
┌─ GC=F · Gold Futures ──────────────── 3,304.20  ▼ -0.8% ───────────────────┐
│ ▲HH·HL ─rng │ 18wk in_band 62% │ QB:bull ●1 Pi·far │ +1.8 cv-neutral ⚠P    │
│ [Overview]  [Structure]  [Cycles]  [Cyber]  [Expectancy]      h/l · Tab    │
├────────────────────────────────────────────────────────────────────────────┤
│  Structure — Daily (252 bars, pivot 5)                 verdict: uptrend,   │
│  ──────────────────────────────────────  repaired May BOS, ext 6% < gate   │
│   Swings        HL  06-02   3,241.10                                       │
│                 HH  05-28   3,388.00                                       │
│                 HL  05-12   3,107.40                                       │
│                 LL  04-30   3,051.00                                       │
│   Break ▼       06-04 broke support 3,180.00 (swing 05-12)                 │
│   MA            20: 3,265 ▲rising · 200: 3,118 ▲rising                     │
│   Extension     +6.0% vs MA200 · rule-13 gate: clear (>20% trips)          │
│                                                                            │
│  Structure — Weekly (104 bars, pivot 3)               verdict: range —     │
│  ──────────────────────────────────────  weekly has not confirmed daily    │
│   Swings        H   05-26   3,388.00 …                                     │
└─ j/k scroll · h/l tab · Esc close ─────────────────────────────────────────┘
```

- **Overview** — the existing popup content unchanged (Asset, Price, Key
  Levels, Chart, classic SMA/BB/RSI/MACD, Portfolio, Thesis, COT, COMEX,
  News) plus G6's Verdicts section. Nothing the operator has today is
  demoted.
- **Structure** — daily block then weekly block, each: verdict line, swing
  table, last BOS events (support + resistance, warning color), MA posture
  with slope glyphs, extension % with the rule-13 gate line (warning color
  when `rule13_extension_gate`, muted "clear" otherwise).
- **Cycles** — `composite_verdict` first; then one block per
  `DegreeStatus`, longest degree first: age `18wk of ~17.4wk exp`, band
  line `in_band 62% · band 15.1-19.3wk (p15-p85, n=11)`, bars-to-band-edge
  countdowns, translation summary (`ledger: RT RT RT LT · rt-string
  intact` / `translation_warning ⚠`), FLD line (`below FLD 3,251 ·
  down-cross 05-30 → target 3,180, 84% achieved, active`), VTL line
  (`intact` / `BROKEN — confirms <break_confirms>` in warning), flag row
  (`failed-cycle · half-cycle-low 05-19 · possible-inversion ·
  small-n`), clarity chip green/amber/red. BTC/gold clock blocks when the
  report carries them.
- **Cyber** — QB state + since-date + bars held; CyberLine value/slope +
  last cross; dot strength `●N` up/down + SuperTrend stop; Pi Cycle
  proximity rendered as two mini-gauges (`top 0.83 ▕███████▏ 1.0`,
  reusing the RSI-gauge pattern); MTF RSI zone with the d/w/m values and
  which timeframes gate; recent dated `SignalEvent`s (newest first,
  component-tagged).
- **Expectancy** — G6's section as a full tab: this asset's
  recently-fired signals joined to `latest_rows` at matching
  `(signal_id, signal_version)`: `n` (nonoverlap), mean lift vs baseline,
  hit lift, MAE mean/worst, significance marker; unmeasured signals get
  the loud `unmeasured — run pftui research backtest` line.

Why B over A: stable spatial memory (FLD is always on Cycles, two
keystrokes from anywhere), bounded render cost per frame, and the pinned
glyph header keeps the G2 verdict-board read visible while drilling — the
popup *opens* where the board left off. The cost — popup-local tab state —
is precedented and trivially testable.

### 6.4 Header vs scroll; glyph conventions

Always visible (pinned, 3 lines): symbol + name + price + day Δ; the
asset's **verdict-board row verbatim** (same glyph vocabulary as G2:
`▲/▼/─` structure, `●N` dots, `⚠P` probation, signed convergence — one
rendering function shared with `verdict_board.rs`, never a second glyph
dialect); the sub-tab bar. Everything else scrolls within its tab.
Color rule unchanged: bull/bear/neutral → gain/loss/muted slots; gates,
breaks, probation, fragility → warning slot; all 11 themes by construction.

### 6.5 Compute contract (off the event loop, always)

These engines cost ~100ms-1s per asset (cycle engine dominates). Render
NEVER calls `analyze`:

- **Held assets:** G1's `IntelSnapshot` already runs all three engines per
  held asset — but keeps only summary fields. G9 widens the snapshot to
  retain the **full** `StructureRead` (D+W), `CycleReport` (all degrees),
  and `CyberSnapshot` (daily; weekly QB line optional behind the same
  budget) per asset. Memory is trivial (structs of dates + decimals);
  compute is identical work G1 already does, minus the discard.
- **Non-held symbols** (watchlist, `/` search): on popup open for a symbol
  absent from the snapshot, push a compute request onto a worker channel
  (sibling of `background_refresh_complete_rx`); tabs render skeleton
  loaders (`widgets/skeleton.rs`) with `computing technicals
  off-thread…` until the result lands via mpsc. Results cached on `App`
  in a small LRU (~16 symbols) keyed `(symbol, history_len)` so reopening
  is instant and a history refresh invalidates naturally.
- **Short series:** engines already return `None`/`Insufficient` below
  their bar minimums (Cyber `MIN_BARS = 60`) — each tab renders the loud
  empty state naming the requirement (`short series — cyber needs ≥ 60
  bars, have 41`), per the EPISTEMICS doctrine.

Popup keys are collision-free by construction: `handle_asset_detail_key`
(app.rs:5318) consumes ALL keys while the popup is open (`_ => {}` arm), so
`h`/`l`/`Tab`/`1`-`5` become sub-tab keys inside the popup without touching
any global binding. Esc still closes.

### 6.6 Stretch — chart overlay of swings/broken levels (separate brief, not a non-goal)

Verdict: **separate stretch brief (G9.3), explicitly not bundled.**
Rationale for keeping it alive: the braille renderer already supports
line overlays (SMA/BB, `price_chart.rs:540-870`), and swing markers +
broken-level lines are the single highest-value visual addition for a
structure-driven operator. Rationale for separating it: it touches the
shared chart widget used by four views (regression surface), needs a
glyph-on-braille-grid alignment solution the engine sections don't, and
G9's operator value (the panel) must not wait on it. It ships only after
G9.1/G9.2 prove the data plumbing.

## 7. The currency rule (anti-recurrence)

This gap exists because a week of substrate work (analyst views, ledgers,
engines, epistemics) shipped CLI/report surfaces by default and the TUI by
omission. The fix is procedural, mirroring DATA-ARCHITECTURE rule 6
(capability briefs): every capability brief that produces an
operator-meaningful output must carry an explicit **Surfaces:** verdict for
report / CLI / TUI / web — "TUI: none (agent-only data)" is a legitimate
answer; silence is not. Tracked in a `docs/SURFACES.md` matrix seeded from
the §2 inventory. See the final TODO brief for the mechanism.
