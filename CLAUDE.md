# CLAUDE.md — Development Guide

> This guide is for AI coding agents making code changes to pftui.

## ⛔ MAXIMUM PRIORITY — DATA SECURITY

**The local pftui installation database on this system contains real personal financial information. It must NEVER be tampered with, exposed, committed, logged, printed, or referenced in the public repo.** This includes:
- Never read, query, or print data from `~/.local/share/pftui/pftui.db` in commits, logs, or output
- Never use real portfolio data in tests, screenshots, demos, or examples — use only synthetic/demo data
- Never commit config files that contain real API keys or personal data
- If you accidentally encounter real financial data, do not include it in any output
> For agent OPERATOR guidance (using pftui as a tool), see [AGENTS.md](AGENTS.md).

## What This Is

**pftui** — a portfolio intelligence platform for human operators and their AI agents. Three interfaces: TUI (terminal), Web Dashboard (browser), CLI (agents/scripts). Backed by SQLite. Written in Rust.

## Documentation Index

| Document | When to Read |
|---|---|
| **[AGENTS.md](AGENTS.md)** | How agents USE pftui (CLI reference, data model, integration patterns) |
| **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** | Code structure, file map, line ranges — READ FIRST before any code change |
| **[docs/ANALYTICS-SPEC.md](docs/ANALYTICS-SPEC.md)** | Feature specifications for analytics, scenarios, journal |
| **[docs/API-SOURCES.md](docs/API-SOURCES.md)** | Free data source reference — endpoints, rate limits, field mappings |
| **[docs/VISION.md](docs/VISION.md)** | Design principles, quality bar, target feature set |
| **[docs/KEYBINDINGS.md](docs/KEYBINDINGS.md)** | Full keyboard shortcut reference |
| **[WEB_DASHBOARD.md](WEB_DASHBOARD.md)** | Web dashboard API schema, setup, deployment |
| **[TODO.md](TODO.md)** | Development backlog — pick tasks from here |
| **[CHANGELOG.md](CHANGELOG.md)** | Release history — append here after completing work |
| **[QA-REPORT.md](QA-REPORT.md)** | Latest QA test results and known bugs |

## Automated Agent Workflow

This repo is improved by automated hourly cron runs. Each run should:

1. **Read state**: `CHANGELOG.md` (what's been done) → `TODO.md` (what to do next)
2. **Pick task**: take the highest-priority unclaimed `[ ]` item from TODO.md
3. **Mark in-progress**: change `[ ]` to `[~]` on the chosen item in TODO.md, commit
4. **Do the work**: implement, adding/updating tests for any logic changes
5. **Test**: run `cargo test` — all tests must pass. Run `cargo clippy` — no new warnings
6. **Update TODO.md**: mark item `[x]`, add any new items discovered during work
7. **Update CHANGELOG.md**: append entry at top of the log section
8. **Commit + push**: one focused commit per task, clear message

**Scoping**: each TODO item is sized for ~1 hour. If a task is bigger, split it into sub-items. If you finish early, pick the next item. Never leave the repo in a broken state (tests failing, partial implementations behind no feature gate).

**Priorities**: P0 (bugs/regressions) → P1 (high-value features/polish) → P2 (nice-to-have) → P3 (speculative). Always work top-down within the highest active priority tier.

## Git

- **Author:** pftui-bot <pftui-bot@users.noreply.github.com>
- Always set both `--author` and `GIT_COMMITTER_NAME`/`GIT_COMMITTER_EMAIL` env vars
- No Co-Authored-By lines — repo doesn't allow co-authors
- **`master` is branch-protected — direct pushes will be rejected.** All changes go through PRs:
  1. `git worktree add /tmp/pftui-work -b "cron/YYYYMMDD-task-name" origin/master`
  2. Work in `/tmp/pftui-work`, commit, push branch
  3. `gh pr create --base master` then `gh pr merge --squash --delete-branch`
  4. Clean up: `cd /root/pftui && git worktree remove /tmp/pftui-work && git pull origin master`
- One focused commit per TODO item
- **REMOVE completed items from TODO.md** — don't mark [x], don't leave them

## Build & Test

```bash
cargo build --release        # binary at target/release/pftui
cargo test                   # run all tests — MUST pass before commit
cargo clippy                 # lint — no new warnings
```

**Always run `cargo test` before committing.** All tests must pass. If you add or change logic, add or update tests. No commit should regress the test suite.

CLI and routine documentation smoke coverage:
- `cargo test --test cli_help_smoke` recursively invokes `--help` across the CLI tree. Any new command or subcommand must keep help rendering side-effect-free.
- `cargo test --test analyst_routine_commands` parses fenced shell blocks in `agents/routines/*.md` and verifies literal `pftui` examples still parse. Run it locally after any routine doc change that touches CLI examples.
- Routine examples are agent contracts. Prefer real, exposed command paths over aspirational syntax; if a workflow has no current CLI writer, document a current read or log command and file a TODO for the missing writer.

## Journal & Notes Authorship

The `journal` and `daily_notes` tables both carry an `author TEXT NOT NULL DEFAULT 'system'` column. Every `pftui journal entry add` and `pftui journal notes add` invocation **must** pass `--author <name>` so writers can be filtered with SQL instead of grep-on-content. The corresponding `list` commands accept `--author <name>` as a filter.

Canonical author identifiers:

| Author | Source |
|---|---|
| `skylar` | Skylar's own journaling |
| `analyst-low` | Low-timeframe analyst routine (hours–days) |
| `analyst-medium` | Medium-timeframe analyst routine (weeks–months) |
| `analyst-high` | High-timeframe analyst routine (quarters–years) |
| `analyst-macro` | Macro-timeframe analyst routine (Dalio/Fourth Turning) |
| `analyst-adversary` | Synthesis-time adversary pseudo-layer (`agents/routines/adversary-analyst.md`) — argues against the four-layer convergence using only the same data |
| `analyst-blind` | Blind control-group analyst (`agents/report-prompts/phase1b-blind-analyst.md`) — raw market data only, no house context; writes views as `--analyst blind` (measurement layer, never votes in convergence) |
| `analyst-antithesis` | Scored rival worldview (`agents/report-prompts/phase2d-antithesis.md`) — maintains the strongest coherent opposite worldview; writes views as `--analyst antithesis` (measurement layer) and predictions scored against the house on the rivalry scoreboard |
| `analyst-evening` | Evening analysis routine |
| `analyst-morning` | Morning brief routine |
| `analyst-newsletter` | Public daily newsletter routine |
| `analyst-night-shift` | Night-shift agent (legacy/optional) |
| `analyst-brief` | Pre-author historical briefs imported from `pftui-operator` |
| `agent-feedback` | AGENT_FEEDBACK.md ingest |
| `system` | Default — anything that didn't specify |

When adding a new agent or routine, mint a new `analyst-*` or `agent-*` identifier (kebab-case, no dots) and update this table.

## Analyst Views & Convergence

The `analyst_views` table records each analyst's structured per-asset view (direction, conviction -5..+5, reasoning, evidence, blind spots, allocation bias). The 4 timeframe-analyst routines populate this for every held position on every run.

Accepted analyst layers: `low|medium|high|macro` (canonical, voting) and `blind|antithesis` (measurement — accepted writers, listed with `layer_class: "measurement"` in `analytics views list --json`, but ALWAYS excluded from convergence voting/averaging, matrices, and report cards). Exclusion happens at the aggregation/loader layer (`build_report_for_asset` and the report BuildContext loaders), never inside `classify_convergence`.

Deterministic convergence aggregation:
- `pftui analytics views convergence --asset <SYM>` — one asset
- `pftui analytics views convergence-all` — every asset with views in window

Summary classifications: `insufficient-views`, `divergent`, `neutral-with-divergence`, `strong-convergent-bull`, `convergent-bull`, `convergent-neutral`, `convergent-bear`, `strong-convergent-bear`. See `src/db/analyst_views.rs::classify_convergence` for the exact formula — do NOT introduce alternative ad-hoc classifications.

## CLI Design Rules

- **Commands navigate, arguments parameterize.** Functions are subcommands in a hierarchy. Data inputs are `--flags`. Never make a parameter a subcommand or a function a flag.
  ```
  # Correct: 'history' and 'add' are commands, '--country' and '--metric' are arguments
  pftui analytics macro cycles history --country US --metric trade
  pftui analytics macro cycles history add --country US --decade 1940 --score 7.5

  # Wrong: 'country' and 'metric' as subcommands
  pftui analytics macro cycles history country US metric trade

  # Wrong: 'history' as a flag
  pftui analytics macro cycles --history --country US
  ```
- **Deep hierarchy over flat namespaces.** Every command lives in a logical tree. No top-level explosion. A user discovers features by walking `--help` down the tree.
- **Canonical domains only.** Top-level CLI domains are `agent`, `analytics`, `data`, `portfolio`, `report`, and `system`. Removed namespaces stay removed.
- **No shortcut aliases that bypass the tree.** If it's an analytical view, it lives under `analytics`. No exceptions.
- **`--json` on every CLI command.** Agents need structured output.
- See PRODUCT-PHILOSOPHY.md principle 9 for full rationale.

## Code Standards

- **rust_decimal for all money** — no f32/f64 for prices, quantities, costs, gains, or allocations. Ever.
- **No `.unwrap()` in production paths** — use `?`, `.unwrap_or_default()`, or `anyhow::bail!`
- **Theme-aware widgets** — every widget reads colors from `app.theme`, never hardcodes colors
- **Stateless render functions** — widgets take `(&mut Frame, Rect, &App)`, no widget state structs
- **anyhow::Result everywhere** — all fallible functions return `Result<T>`
- **Decimal strings in SQLite** — store monetary values as TEXT, parse with `Decimal::from_str`
- **Vim-native keybindings** — follow vim conventions: j/k navigate, gg/G jump, / search, Esc cancel
- **`--json` on every CLI command** — agents need structured output

## Schema Migration Contract

- Prior-release SQLite compatibility is guarded by `cargo test --test prior_release_schema`.
- Any PR adding an `ALTER TABLE` migration must check whether `tests/fixtures/db/v0.27.0.sqlite` needs an old-state table/column shape update.
- Use only synthetic/demo data in schema fixtures. Never copy from a real local portfolio DB.
- If startup reports a missing-column schema error, run `pftui system schema verify`, then `pftui system schema repair --dry-run`, then `pftui system schema repair --confirm` after reviewing the plan. The schema command opens SQLite before normal migrations so it can diagnose drift that would otherwise be auto-mutated or fail during startup.

## Architecture Quick Reference

**Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full file map with line ranges.**

### Key Patterns

| Task | Where to Look |
|---|---|
| Fix keybinding | `app.rs:1398-1733` (`handle_key()`) |
| Add CLI command | `cli.rs` + `commands/new.rs` + `main.rs` |
| Add TUI view/tab | `app.rs:1-285` (ViewMode enum) + `ui.rs` + `help.rs` + `header.rs` |
| Add widget | `tui/widgets/new.rs` + parent view + `widgets/mod.rs` |
| Fix chart | `price_chart.rs` render + `app.rs:1139-1314` variant logic |
| Theme changes | `theme.rs` (all 11 themes — update ALL of them) |
| Price fetching | `price/yahoo.rs` or `price/coingecko.rs` |
| Add data source | `src/data/new.rs` + `src/db/new_cache.rs` + wire into `refresh.rs` |

### Module Structure

```
src/
  main.rs           — CLI dispatch
  app.rs            — TUI state, keybindings, tick loop (6000 lines — use line ranges)
  cli.rs            — clap CLI definitions
  commands/         — CLI command implementations
  db/               — SQLite schema, CRUD operations
  models/           — Data structs (Position, Transaction, Asset)
  price/            — Price fetching (Yahoo, CoinGecko)
  data/             — External data sources (COT, BLS, RSS, Polymarket, etc.)
  tui/
    views/          — Tab views (positions, markets, economy, etc.)
    widgets/        — Reusable UI components
  indicators/       — Technical indicators (RSI, MACD, SMA, Bollinger)
  regime/           — Market regime detection
  web/              — Web dashboard server
```

## Never

- Modify `README.md` or anything in `website/` only with explicit approval from the maintainers
- Break existing keybindings
- Use floats (f32/f64) for financial data
- Skip theme support on new widgets (use `app.theme.*`)
- Use `.unwrap()` in production code paths
- Store monetary values as floats in SQLite
- Skip privacy mode support on new views
- Add blocking I/O to the TUI event loop thread
- Leave TODO.md with a `[~]` item and no matching commit
- Push with failing tests
- Add dependencies without clear justification
