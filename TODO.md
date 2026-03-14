# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P1 — Feature Requests

### F39: Macro Analytics Consolidation

Routing and composite score shipped. Remaining:

### F40: CLI Hierarchy Restructure

> Collapse 55 top-level commands into ~15 with logical groupings.
> Old commands become deprecated aliases (print warning, forward to new path).
> No functionality changes. Pure routing refactor.
> See PRODUCT-PHILOSOPHY.md principle 9 and CLAUDE.md CLI Design Rules.

**F40.9: Convert all positional `<ACTION>` to proper clap subcommands**

Every command that currently uses `<ACTION>` as a positional string must be converted
to proper clap `Subcommand` enums. This gives each action its own `--help` with only
relevant flags. Applies to all commands restructured in F40.3 and F40.4.

Source: `src/cli.rs` (refactor each into nested Subcommand enum), `src/main.rs` (dispatch).

**Final top-level tree after F40:**
```
pftui
├── portfolio        # your money: holdings, transactions, targets, rebalancing
├── watchlist        # tracked symbols
├── journal          # your thinking: predictions, convictions, notes, scenarios
├── analytics        # the engine: timeframes, trends, alerts, research, regime
├── market           # external data: news, sentiment, calendar, fedwatch
├── dashboard        # pre-built views: macro, oil, crisis, sector
├── data             # data management: refresh, status
├── agent            # agentic operations: messaging, digest, pipeline
├── system           # admin: config, doctor, export, setup
└── help
```
55 top-level → 9 + help. Every grouping navigable via `--help`.

### Alignment Scoring Algorithm

### Data Source Reliability

8/10 sources stale, price_history writes stopped. Must stabilize.

---

## P2 — Nice to Have


---

## P3 — Long Term

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
