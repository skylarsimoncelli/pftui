# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 — Feature Requests

### [Feedback] Daily P&L Dollar Column in TUI Positions View

Add a `Day P&L $` column to the main positions table (Tab 1). This is the single most requested feature across ALL Sentinel TUI reviews (6 consecutive reviews). Portfolio managers need to see actual dollar impact per position immediately, not just percentages. For a $370k portfolio, daily movements of $5-15k are routine and need immediate visibility.

Source: `src/tui/views/positions.rs`. Use `price_history` day-over-day delta × quantity for each position.

### [Feedback] Sector/Category Grouping in Positions View

Add toggle or secondary view showing positions grouped by asset class (Cash, Commodities, Crypto, Equities) with subtotals per group. A macro investor thinks in asset classes, not individual symbols. Include group-level allocation %, P&L, and optional sparklines.

Source: `src/tui/views/positions.rs`. Leverage existing `category` field on transactions.

### [Feedback] Data Source Conflict Detection

When multiple data sources provide contradictory data (e.g., 92% hold vs 98.9% cut probability from different FOMC sources), the system should flag the conflict and suggest the most reliable source based on track record. Currently wrong data cascades across the agent pipeline unchecked.

Source: `src/commands/refresh.rs`, `src/commands/fedwatch.rs`. Add conflict detection layer that compares values from multiple sources for the same metric.

### [Feedback] `predict score` Positional Argument Syntax

`pftui predict score 51 correct notes` fails with exit code 2 — users must discover `--id`/`--outcome`/`--notes` flag syntax from `--help`. The positional variant should work for faster scripting. At minimum, improve the error message to show correct syntax.

Source: `src/cli.rs` (journal prediction score subcommand), `src/commands/predict.rs`.

### [Feedback] `correlations latest` Command

`correlations latest` fails. Need a simpler snapshot command for understanding current asset relationships without specifying symbols or history windows. A quick `pftui analytics correlations latest` should show the most recent stored correlation snapshot.

Source: `src/commands/correlations.rs`.

### [Feedback] Today-Only Alert Filtering

`pftui analytics alerts` should support a `--today` flag to show only alerts triggered since midnight, filtering out historical noise. During market close routines, agents need to focus on fresh signals only.

Source: `src/commands/alerts.rs`, `src/alerts/engine.rs`.

### F39: Macro Analytics Consolidation (Remaining)

Routing and composite score shipped. Remaining alignment scoring algorithm work.

### Data Source Reliability

8/10 sources stale, price_history writes stopped. Must stabilize.

---

## P2 — Nice to Have

### [Feedback] `predict add` Timeframe Parameter Handling

`predict add` rejects the `timeframe` param despite it being documented. Improve error messaging or fix parameter acceptance. Also consider adding `--confidence` flag for prediction confidence scoring.

Source: `src/cli.rs`, `src/commands/predict.rs`.

### [Feedback] `scenario update --notes` Inline Annotation

`scenario update` with `--notes` flag errors with "unexpected argument" (reported Mar 10, Mar 13). The `--notes` alias to `--driver` for history logging should be verified as working end-to-end.

Source: `src/commands/scenario.rs`, `src/cli.rs`.

### [Feedback] `conviction set` Ergonomics for Negative Scores

`scenario update` requires a separate `--driver` flag instead of accepting inline notes like `conviction set` does. Make these commands more ergonomically consistent.

Source: `src/commands/scenario.rs`, `src/commands/conviction.rs`.

### [Feedback] Scan Alert for Trackline Breaches

Add quick detection of technical level breaches (e.g., silver breaking $83 SMA50 support) to scan alerts. Would enable real-time positioning signals.

Source: `src/alerts/engine.rs`, `src/commands/scan.rs`.

### [Feedback] `agent-msg send` Batch Improvements

Agent-msg batch mode works but could benefit from grouped intel package semantics — multiple related messages sent as a single logical unit with shared context.

Source: `src/commands/agent_msg.rs`.

---

## P3 — Long Term

### F40: CLI Hierarchy Restructure (Cleanup Phase)

Core namespace restructure shipped (portfolio, market, system, dashboard, data, agent, watchlist, journal, analytics all routed). Remaining:
- [x] Remove legacy top-level aliases after deprecation period
- [x] Update all agent routine docs to use new paths exclusively
- [x] Update README examples to new command paths

### F41: Interactive Shell (`pftui shell`)

> A human-friendly interactive CLI session with autocompletion and command discovery.
> Not a TUI with panels. A shell. Think Cisco IOS, Redis CLI, or MySQL monitor.

```
$ pftui shell
pftui> analytics macro
  compare      Head-to-head power comparison between two countries
  cycles       Big Cycle and Fourth Turning stage tracking
  history      Historical power metrics by decade
  log          Weekly structural observations
  metrics      Dalio 8 determinants for a country
  outcomes     Structural outcome probabilities
  parallels    Historical parallel tracker
  regime       Market regime classification

pftui> analytics macro cycles history --country US --decade 1940
[output]

pftui> journal prediction scorecard --date yesterday
[output]
```

Features:
- Tab completion at every level of the command tree
- `?` or partial command shows available subcommands with descriptions (IOS-style)
- Command history (readline/rustyline with persistent history file)
- Context-aware: after typing `journal`, only journal subcommands complete
- Colored output, same as CLI
- `exit` or Ctrl-D to quit
- Optional: `enable` mode for destructive operations (add, remove, update)

Implementation: `rustyline` crate for readline. Build completer from the clap
command tree (clap already knows the full hierarchy). Each command's `about` text
becomes the description shown in the library view.

Low priority. The deep CLI hierarchy must ship first (F40) since this shell
is built on top of it. The better the tree, the better the shell.

Source: new `src/commands/shell.rs`, `src/cli.rs` (add shell subcommand).

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

---

## Feedback Summary

**Latest Scores Per Tester (most recent review):**

| Tester | Latest Score | Date | Trend |
|--------|-------------|------|-------|
| Morning Market Research | 15% | Mar 8 | ↓ (DB crash — likely fixed) |
| Evening Eventuality Planner | 55% | Mar 9 | ↓ (hang bug — likely fixed) |
| Sentinel Main (TUI) | 72% | Mar 10 | → (stable 72-88% range since Mar 3) |
| Market Close | 72% | Mar 9 | → (oscillating 68-88%) |
| UX Analyst | 75% | Mar 8 | → (stable 68-78% range) |
| Integration Optimiser | 70% | Mar 11 | — (single review) |

**Score Trend Notes:**
- Morning Research and Evening Planner had catastrophic drops on Mar 8-9 due to the `group_id` DB migration bug and API timeout hangs. Both bugs were fixed (graceful degradation, connection timeouts, `--offline` mode all shipped). Their scores should recover to 78-85% range on next review.
- Sentinel TUI has plateaued in the 72-88% range. The consistent #1 ask is daily P&L $ in the positions table.
- Market Close peaked at 88% (Mar 6) when data pipeline was fully working, dropped to 72% when movers/daily-change calculation broke. Fix was shipped.

**Top 3 Priorities Based on Feedback:**

1. **Daily P&L $ column in TUI positions** — Requested by Sentinel in 6/8 reviews. Table stakes for portfolio management. Blocks Sentinel from breaking above 85%.
2. **Sector/category grouping in positions** — Requested by Sentinel (×4) and Market Close (×2). Macro investors think in asset classes.
3. **Data source reliability** — While many fixes shipped (timeouts, graceful degradation, `--offline`), the 15% and 55% scores show that data pipeline failures are catastrophic for agent trust. Continued hardening needed.

**Release Assessment:**
- Significant work has landed since v0.9.0: full F40 CLI hierarchy restructure (9 new namespaces), F39.7 macro history, clippy fixes, journal subcommand conversions.
- No P0 bugs remain. `cargo test` passes (1199 tests). `cargo clippy --all-targets -- -D warnings` passes clean.
- **Release v0.10.0 is warranted** — the F40 CLI restructure is a major UX change deserving a minor version bump.
- GitHub stars: 0 — Homebrew Core submission not yet eligible (requires 50+).
