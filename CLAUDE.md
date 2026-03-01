# CLAUDE.md — pftui

## What This Is

**pftui** — the most comprehensive, full-featured, aesthetic terminal UI portfolio & market tracker ever built. Bloomberg Terminal meets btop. Live market data, braille charts, 6 themes, ratio analysis, privacy mode. Rust + ratatui.

The goal: the best possible terminal experience for viewing your investment portfolio, with live market data, news, economic indicators. Vim-native, visually stunning, information-dense, intuitive.

## Automated Agent Workflow

This repo is improved by automated hourly cron runs. Each run should:

1. **Read state**: `CHANGELOG.md` (what's been done) → `TODO.md` (what to do next)
2. **Pick task**: take the highest-priority unclaimed `[ ]` item from TODO.md
3. **Mark in-progress**: change `[ ]` to `[~]` on the chosen item in TODO.md, commit
4. **Do the work**: implement, adding/updating tests for any logic changes
5. **Test**: run `cargo test` — all tests must pass. Run `cargo clippy` — no new warnings
6. **Update TODO.md**: mark item `[x]`, add any new items discovered during work
7. **Update CHANGELOG.md**: append entry at top of the log section
8. **Update docs/README.md**: if you added features, keybindings, views, or config options
9. **Commit + push**: one focused commit per task, clear message

**Scoping**: each TODO item is sized for ~1 hour. If a task is bigger, split it into sub-items. If you finish early, pick the next item. Never leave the repo in a broken state (tests failing, partial implementations behind no feature gate).

**Priorities**: P0 (bugs/regressions) → P1 (high-value features/polish) → P2 (nice-to-have) → P3 (speculative). Always work top-down within the highest active priority tier.

## Git

- **Author:** Skylar Simoncelli <skylar@skylar.email>
- Always set both `--author` and `GIT_COMMITTER_NAME`/`GIT_COMMITTER_EMAIL` env vars
- No Co-Authored-By lines — repo doesn't allow co-authors
- Never merge branches or PRs
- One focused commit per TODO item

## Build & Test

```bash
cargo build --release        # binary at target/release/pftui
cargo test                   # run all tests — MUST pass before commit
cargo clippy                 # lint — no new warnings
```

**Always run `cargo test` before committing.** All tests must pass. If you add or change logic (chart variants, position computation, DB operations, price routing, keybindings, etc.), add or update tests to cover it. No commit should regress the test suite.

## Code Standards

- **rust_decimal for all money** — no f32/f64 for prices, quantities, costs, gains, or allocations. Ever.
- **No `.unwrap()` in production paths** — use `?`, `.unwrap_or_default()`, or `anyhow::bail!`
- **Theme-aware widgets** — every widget reads colors from `app.theme`, never hardcodes colors
- **Stateless render functions** — widgets take `(&mut Frame, Rect, &App)`, no widget state structs
- **anyhow::Result everywhere** — all fallible functions return `Result<T>`
- **Decimal strings in SQLite** — store monetary values as TEXT, parse with `Decimal::from_str`
- **Vim-native keybindings** — follow vim conventions: j/k navigate, gg/G jump, / search, Esc cancel, etc.

## Architecture Rules

- All widgets access `app.theme` for colors — no exceptions
- New views must support privacy mode (`is_privacy_view(app)` check)
- Price sources need fallback (CoinGecko → Yahoo for crypto, Yahoo primary for everything else)
- Charts support the variant system (Single, Ratio, All) — new chart types must integrate with `J`/`K` cycling
- Price service runs on a dedicated thread with Tokio runtime, communicates via channels
- TUI event loop runs at ~60fps (16ms tick) — never block it
- Cash assets always price at 1.0 — never fetch prices for cash
- New views/tabs: add to ViewMode enum, wire into `handle_key`, add number key shortcut, update help overlay

## Key File Map

```
src/
  main.rs                    — CLI dispatch, startup flow
  app.rs                     — App state, keybindings, tick loop, chart variant logic
  cli.rs                     — clap CLI definitions
  config.rs                  — config.toml load/save
  commands/
    setup.rs                 — interactive setup wizard (full + percentage modes)
    add_tx.rs                — add transaction (interactive or flags)
    remove_tx.rs             — delete transaction by ID
    list_tx.rs               — print transactions to stdout
    export.rs                — CSV/JSON export
    summary.rs               — portfolio summary to stdout
  db/
    schema.rs                — SQLite migrations (4 tables)
    transactions.rs          — transaction CRUD
    price_cache.rs           — spot price cache CRUD
    price_history.rs         — daily history CRUD
    allocations.rs           — percentage mode allocations CRUD
  models/
    position.rs              — Position struct, compute_positions(), compute_positions_from_allocations()
    transaction.rs           — Transaction, NewTransaction, TxType
    allocation.rs            — Allocation struct
    asset.rs                 — AssetCategory, PriceProvider enums
    asset_names.rs           — ~130 symbol name map, infer_category(), search_names()
    price.rs                 — PriceQuote, HistoryRecord structs
    portfolio.rs             — PortfolioSummary struct
  price/
    mod.rs                   — PriceService (thread + channels), PriceCommand/PriceUpdate
    yahoo.rs                 — Yahoo Finance API (spot + history)
    coingecko.rs             — CoinGecko API (spot + history), 62-coin ID map, Yahoo fallback
  tui/
    mod.rs                   — terminal setup, run loop
    event.rs                 — EventHandler thread (Key, Tick, Resize)
    theme.rs                 — Theme struct (28 color slots), 6 themes, gradients, animations
    ui.rs                    — root layout compositor
    views/
      positions.rs           — positions table (full + privacy variants)
      transactions.rs        — transactions table
      help.rs                — help overlay popup
    widgets/
      header.rs              — top bar (logo, tabs, value, clock)
      status_bar.rs          — bottom bar (key hints, live indicator)
      sidebar.rs             — sidebar compositor (allocations + sparkline)
      allocation_bars.rs     — category allocation horizontal bars
      portfolio_sparkline.rs — 90d portfolio braille sparkline
      price_chart.rs         — per-position braille price/ratio charts
```

## Never

- Break existing keybindings
- Add dependencies without clear justification
- Use floats (f32/f64) for financial data
- Skip theme support on new widgets
- Hardcode colors — always use `app.theme.*` fields
- Use `.unwrap()` in production code paths
- Store monetary values as floats in SQLite
- Skip privacy mode support on new views
- Add blocking I/O to the TUI event loop thread
- Leave TODO.md with a `[~]` item and no matching commit (never abandon mid-task)
- Push with failing tests
