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

**F40.1: `pftui portfolio` namespace**

Move portfolio-related commands under `portfolio`:
```
pftui portfolio summary              # was: pftui summary
pftui portfolio value                # was: pftui value
pftui portfolio performance          # was: pftui performance
pftui portfolio history              # was: pftui history
pftui portfolio brief                # was: pftui brief
pftui portfolio eod                  # was: pftui eod
pftui portfolio target               # was: pftui target
pftui portfolio drift                # was: pftui drift
pftui portfolio rebalance            # was: pftui rebalance
pftui portfolio stress-test          # was: pftui stress-test
pftui portfolio dividends            # was: pftui dividends
pftui portfolio annotate             # was: pftui annotate
pftui portfolio group                # was: pftui group
pftui portfolio opportunity          # was: pftui opportunity
pftui portfolio set-cash             # was: pftui set-cash
pftui portfolio transaction add      # was: pftui add-tx
pftui portfolio transaction remove   # was: pftui remove-tx
pftui portfolio transaction list     # was: pftui list-tx
```
`pftui portfolio` with no subcommand shows `summary` (default view).
Source: `src/cli.rs` (add Portfolio subcommand enum), `src/main.rs` (dispatch).
Keep old top-level commands as deprecated aliases.

**F40.3: `pftui journal` as unified knowledge layer**

All recorded thinking lives under `journal`:
```
pftui journal                              # overview / recent entries across all types
pftui journal entry add                    # was: pftui journal add (free-form entries)
pftui journal entry list                   # was: pftui journal list
pftui journal entry search                 # was: pftui journal search
pftui journal entry update                 # was: pftui journal update
pftui journal entry remove                 # was: pftui journal remove
pftui journal entry tags                   # was: pftui journal tags
pftui journal entry stats                  # was: pftui journal stats
pftui journal prediction add               # was: pftui predict add
pftui journal prediction list              # was: pftui predict list
pftui journal prediction score             # was: pftui predict score
pftui journal prediction stats             # was: pftui predict stats
pftui journal prediction scorecard         # was: pftui predict scorecard
pftui journal conviction set               # was: pftui conviction set
pftui journal conviction list              # was: pftui conviction list
pftui journal conviction history           # was: pftui conviction history
pftui journal conviction changes           # was: pftui conviction changes
pftui journal notes add                    # was: pftui notes add
pftui journal notes list                   # was: pftui notes list
pftui journal notes search                 # was: pftui notes search
pftui journal notes remove                 # was: pftui notes remove
pftui journal scenario add                 # was: pftui scenario add
pftui journal scenario list                # was: pftui scenario list
pftui journal scenario update              # was: pftui scenario update
pftui journal scenario remove              # was: pftui scenario remove
pftui journal scenario history             # was: pftui scenario history
pftui journal scenario signal add          # was: pftui scenario signal-add
pftui journal scenario signal list         # was: pftui scenario signal-list
pftui journal scenario signal update       # was: pftui scenario signal-update
pftui journal scenario signal remove       # was: pftui scenario signal-remove
```
Source: `src/cli.rs` (Journal subcommand with nested Prediction, Conviction,
Notes, Scenario sub-enums), `src/main.rs` (dispatch).

**F40.4: `pftui analytics` absorbs analytical tools**

```
pftui analytics summary                          # unchanged
pftui analytics low / medium / high              # unchanged
pftui analytics alignment / divergence           # unchanged
pftui analytics digest / recap                   # unchanged
pftui analytics movers                           # was: pftui movers
pftui analytics correlations                     # was: pftui correlations
pftui analytics scan                             # was: pftui scan
pftui analytics research                         # was: pftui research
pftui analytics trends add                       # was: pftui trends add
pftui analytics trends list                      # was: pftui trends list
pftui analytics trends update                    # was: pftui trends update
pftui analytics trends evidence add              # was: pftui trends evidence-add
pftui analytics trends evidence list             # was: pftui trends evidence-list
pftui analytics trends impact add                # was: pftui trends impact-add
pftui analytics trends impact list               # was: pftui trends impact-list
pftui analytics trends dashboard                 # was: pftui trends dashboard
pftui analytics alerts add                       # was: pftui alerts add
pftui analytics alerts list                      # was: pftui alerts list
pftui analytics alerts remove                    # was: pftui alerts remove
pftui analytics alerts check                     # was: pftui alerts check
pftui analytics alerts ack                       # was: pftui alerts ack
pftui analytics alerts rearm                     # was: pftui alerts rearm
pftui analytics macro metrics                    # unchanged
pftui analytics macro compare                    # unchanged
pftui analytics macro cycles                     # unchanged
pftui analytics macro outcomes                   # unchanged
pftui analytics macro parallels                  # unchanged
pftui analytics macro log                        # unchanged
pftui analytics macro regime current             # was: pftui regime current
pftui analytics macro regime history             # was: pftui regime history
pftui analytics macro regime transitions         # was: pftui regime transitions
```
Source: `src/cli.rs`, `src/commands/analytics.rs`.

**F40.5: `pftui market` namespace**

Group external market data:
```
pftui market news                # was: pftui news
pftui market sentiment           # was: pftui sentiment
pftui market calendar            # was: pftui calendar
pftui market fedwatch            # was: pftui fedwatch
pftui market economy             # was: pftui economy
pftui market predictions         # was: pftui predictions (Polymarket)
pftui market options             # was: pftui options
pftui market etf-flows           # was: pftui etf-flows
pftui market supply              # was: pftui supply
pftui market sovereign           # was: pftui sovereign
```
Source: `src/cli.rs`, `src/main.rs`.

**F40.8: `pftui system` namespace**

Admin/system commands:
```
pftui system config              # was: pftui config
pftui system db-info             # was: pftui db-info
pftui system doctor              # was: pftui doctor
pftui system export              # was: pftui export
pftui system import              # was: pftui import
pftui system snapshot            # was: pftui snapshot
pftui system setup               # was: pftui setup
pftui system demo                # was: pftui demo
pftui system web                 # was: pftui web
pftui system migrate-journal     # was: pftui migrate-journal
```
Source: `src/cli.rs`, `src/main.rs`.

**F40.9: Convert all positional `<ACTION>` to proper clap subcommands**

Every command that currently uses `<ACTION>` as a positional string must be converted
to proper clap `Subcommand` enums. This gives each action its own `--help` with only
relevant flags. Applies to all commands restructured in F40.3 and F40.4.

Source: `src/cli.rs` (refactor each into nested Subcommand enum), `src/main.rs` (dispatch).

**F40.10: Deprecated alias system**

All old top-level commands must continue working with a deprecation warning:
```
$ pftui macro
Warning: `pftui macro` is deprecated. Use `pftui dashboard macro` instead.
[normal output follows]
```
Implementation: match old command names in `src/main.rs`, print warning to stderr,
forward to new dispatch. Remove aliases after 3 major versions.
`structural` already uses this pattern -- reuse the same mechanism.

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
