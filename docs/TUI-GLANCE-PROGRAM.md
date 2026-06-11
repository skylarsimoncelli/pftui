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

## 5. The currency rule (anti-recurrence)

This gap exists because a week of substrate work (analyst views, ledgers,
engines, epistemics) shipped CLI/report surfaces by default and the TUI by
omission. The fix is procedural, mirroring DATA-ARCHITECTURE rule 6
(capability briefs): every capability brief that produces an
operator-meaningful output must carry an explicit **Surfaces:** verdict for
report / CLI / TUI / web — "TUI: none (agent-only data)" is a legitimate
answer; silence is not. Tracked in a `docs/SURFACES.md` matrix seeded from
the §2 inventory. See the final TODO brief for the mechanism.
