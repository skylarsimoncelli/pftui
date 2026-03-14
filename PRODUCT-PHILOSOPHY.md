# Product Philosophy

## Core Beliefs

### 1. The best financial tool is one that two intelligences operate together

A human alone misses things. They get emotional. They check markets sporadically. They forget what they said last week about gold.

An AI agent alone lacks judgment. It can't feel when a trade is wrong. It doesn't have skin in the game. It doesn't know when to override the data.

Together, they're better than either alone. The human provides conviction, context, and decision authority. The agent provides discipline, memory, research depth, and tireless monitoring. pftui is the shared workspace where they collaborate.

### 2. Data should be centralised, not scattered

Every financial data source you check is a context switch. Every web search an agent runs is a round-trip that could fail, change format, or go stale. The right architecture is: pull all data into one local store, serve it through one tool, and let both operators work from the same truth.

pftui's `refresh` command fetches from 10+ sources in one call. Its SQLite database is the single source of truth. Its CLI serves everything the agent needs. Its TUI and web dashboard serve everything the human needs. One tool, one database, zero duplication.

### 3. The agent should be the primary operator

This is counterintuitive. Most tools are built for humans with agent compatibility bolted on. pftui inverts this: the agent is the primary daily operator, and the TUI is what the human opens when they want to go deeper.

The agent refreshes data, monitors for threshold breaches, assembles briefs, tracks narratives, logs analysis, and delivers insights. The human reads the output, asks questions, makes decisions, and provides the conviction signals the agent can't generate alone.

This means:
- Every feature ships with a CLI command and `--json` output
- The database schema is agent-readable and documented
- AGENTS.md is as thorough as any user manual
- The recommended setup path is "tell your agent to set this up"

### 4. Zero-config, zero-key by default

Financial data should not require a credit card, an API key signup, or a subscription. pftui's core functionality works with zero configuration — install and go. Yahoo Finance, CoinGecko, Polymarket, CFTC, BLS, RSS feeds, and more — all free, all keyless.

Optional API keys (Brave Search, FRED) unlock enhanced capabilities. But the tool is complete without them. This is a hard product requirement: a user should be able to install pftui and get a fully functional financial intelligence platform in under a minute.

### 5. Local-first is non-negotiable

Your portfolio data — what you hold, what you paid, what you're watching — is yours. It lives on your machine in a SQLite file. It never touches a cloud server. It never requires an account.

Privacy mode (`p` key) instantly masks all monetary values. Percentage-only mode stores no dollar amounts at all. You can screen-share, stream, or screenshot without exposing your net worth.

This isn't just a feature — it's a philosophical commitment. Financial sovereignty means controlling your own data.

### 6. Density over simplicity

pftui is maximalist. Bloomberg Terminal information density, not Robinhood simplicity. Every terminal cell earns its place. If a panel has empty space, it should show more data.

This serves both operators:
- The human sees their full financial picture in one screen — no scrolling, no clicking, no loading
- The agent gets comprehensive data in one JSON blob — no pagination, no multi-step queries

The learning curve is intentional. This is a power tool, not a toy. Vim keybindings, 7 views, 11 themes, braille charts — the interface rewards investment.

### 7. Memory compounds

A financial tool that forgets is just a calculator. pftui is designed for accumulation:
- The SQLite database persists across sessions
- Price history builds daily
- The journal tracks every decision and outcome
- The agent writes analysis to persistent markdown files
- Scenarios evolve as evidence accumulates
- Accuracy tracking improves the system over time

Day 1 of pftui is a portfolio tracker. Day 100 is a personalised financial intelligence system with a thesis, scenario map, accuracy record, and institutional memory. The tool's value grows with every day it runs.

### 8. The install script is the front door

Re-running the install script upgrades to the latest version. This is the canonical install and update path. No package manager lock-in, no version pinning, no dependency hell. One command to install, the same command to upgrade.

The recommended experience: "Ask your AI agent (Claude Code, Codex, or OpenClaw) to install and set up pftui for you." The agent runs the script, reads AGENTS.md, and guides the human through setup. This is the golden path.

## Design Decisions (and why)

### Why Rust?
Single binary. No runtime. No dependencies. Fast enough for 60fps TUI rendering with real-time price updates. Safe enough for financial calculations (rust_decimal, no floating point). Compiles everywhere.

### Why SQLite?
Zero-config. Ships with the binary. No database server. Works offline. Fast enough for pftui's use case. Agents can query it directly. PostgreSQL support is planned for multi-agent deployments, but SQLite is the right default.

### Why terminal UI?
Developers and power users live in the terminal. A TUI starts instantly, uses zero memory compared to Electron, works over SSH, and integrates with existing workflows (tmux, screen, dotfiles). The web dashboard serves mobile/remote use cases without replacing the TUI as the primary interface.

### Why not real-time streaming?
pftui refreshes on-demand. Prices are "live enough" for portfolio management — you're tracking positions over days and weeks, not scalping. Real-time WebSocket support is planned but not prioritised because the core user is a swing/position trader, not a day trader.

### Why free data sources?
Barrier to entry. If someone has to sign up for 3 API keys before seeing their portfolio, they'll close the tab. pftui works out of the box with Yahoo Finance, CoinGecko, and a constellation of free APIs. Optional keys enhance the experience without gating it.

### Why Brave Search API as the premium upgrade?
One API, one key, covers everything: news, economic data, research, earnings, geopolitics. Instead of maintaining 10 fragile scrapers (each with their own failure modes), Brave gives us a single reliable data source that can answer ANY financial question. The free tier ($5/month in credits) is more than enough for daily use.

### 9. Deep command hierarchy over flat namespaces

pftui follows Cisco IOS-style CLI design: long, navigable, hierarchical commands over short ambiguous ones. The CLI is an operating system for financial intelligence, and it should feel like one.

```
# Yes: structured, navigable, discoverable
pftui analytics macro history US --metric trade
pftui analytics macro compare US China
pftui agent journal prediction scorecard --date 2026-03-13 --json

# No: flat, ambiguous, guesswork
pftui history US trade
pftui compare US China
pftui scorecard low
```

Principles:
- **Hierarchy over aliases.** Every command lives in a logical tree. `analytics` contains `low`, `medium`, `high`, `macro`, `alignment`, `divergence`. `macro` contains `metrics`, `compare`, `cycles`, `history`. A user can explore by tab-completing down the tree.
- **No top-level explosion.** Top-level commands should be countable on two hands. Everything else nests. A flat namespace with 60 top-level commands is a search problem, not a CLI.
- **Self-documenting depth.** `pftui analytics macro --help` shows you the macro sub-universe. You don't need to read docs to discover what exists.
- **Verbosity is a feature.** A long command that reads like a sentence (`pftui analytics macro history US China --metric military --decade 1940`) is better than a short command that requires a man page (`pftui amh US CN -m mil -d 1940`).
- **Consistency over brevity.** If `analytics` is the namespace for analytical views, ALL analytical views live there. No shortcuts that bypass the hierarchy.

This compounds with agent usage: an agent can discover the full command tree by walking `--help` at each level. Flat namespaces require documentation or trial-and-error.

## What pftui Will Never Be

- **A trading platform.** pftui is read-only. It tracks, analyses, and advises. It never executes trades or moves money. The human is always the decision-maker.

- **A web-first app.** The terminal is the primary interface. The web dashboard is supplementary. We will never add features to the web that don't exist in the TUI and CLI.

- **A social platform.** No leaderboards, no sharing, no community features. Your portfolio is private. Your data is yours.

- **A subscription service.** pftui is open source, MIT licensed, free forever. Optional API keys are between you and the key provider (Brave, FRED, etc.), not us.

- **Simple.** If you want a simple portfolio tracker, use a spreadsheet. pftui is for people who want depth, density, and power. The complexity is the point.
