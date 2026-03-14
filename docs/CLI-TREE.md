# CLI Tree

Canonical `pftui` command tree after F42. This document must match `--help` output.

## Rules

- Only five top-level domains are supported: `agent`, `analytics`, `data`, `portfolio`, `system`.
- Commands navigate. Flags parameterize.
- If an operation has multiple actions, actions are subcommands, not positional strings.
- Removed paths are removed. There are no compatibility aliases.
- Every data-returning command must continue to support `--json`.

## Top Level

```text
pftui
├── agent
│   ├── message
│   │   ├── send
│   │   ├── list
│   │   ├── reply
│   │   ├── flag
│   │   ├── ack
│   │   ├── ack-all
│   │   └── purge
│   └── journal
│       ├── entry
│       ├── prediction
│       ├── conviction
│       ├── notes
│       └── scenario
├── analytics
│   ├── signals
│   ├── summary
│   ├── low
│   ├── medium
│   ├── high
│   ├── macro
│   │   ├── metrics
│   │   ├── compare
│   │   ├── cycles
│   │   ├── outcomes
│   │   ├── parallels
│   │   ├── log
│   │   └── regime
│   │       ├── current
│   │       ├── history
│   │       └── transitions
│   ├── alignment
│   ├── divergence
│   ├── digest
│   ├── recap
│   ├── gaps
│   ├── movers
│   ├── correlations
│   │   ├── compute
│   │   └── history
│   ├── scan
│   ├── research
│   ├── trends
│   │   ├── add
│   │   ├── list
│   │   ├── update
│   │   ├── dashboard
│   │   ├── evidence
│   │   │   ├── add
│   │   │   └── list
│   │   └── impact
│   │       ├── add
│   │       └── list
│   └── alerts
│       ├── add
│       ├── list
│       ├── remove
│       ├── check
│       ├── ack
│       └── rearm
├── data
│   ├── refresh
│   ├── status
│   ├── dashboard
│   │   ├── macro
│   │   ├── oil
│   │   ├── crisis
│   │   ├── sector
│   │   ├── heatmap
│   │   └── global
│   ├── news
│   ├── sentiment
│   ├── calendar
│   ├── fedwatch
│   ├── economy
│   ├── predictions
│   ├── options
│   ├── etf-flows
│   ├── supply
│   └── sovereign
├── portfolio
│   ├── summary
│   ├── value
│   ├── brief
│   ├── eod
│   ├── performance
│   ├── history
│   ├── target
│   │   ├── set
│   │   ├── list
│   │   └── remove
│   ├── drift
│   ├── rebalance
│   ├── stress-test
│   ├── dividends
│   ├── annotate
│   ├── group
│   ├── opportunity
│   │   ├── add
│   │   ├── list
│   │   └── stats
│   ├── profiles
│   │   ├── list
│   │   ├── current
│   │   ├── create
│   │   ├── switch
│   │   └── remove
│   ├── watchlist
│   │   ├── add
│   │   ├── remove
│   │   └── list
│   ├── set-cash
│   └── transaction
│       ├── add
│       ├── remove
│       └── list
└── system
    ├── config
    ├── db-info
    ├── doctor
    ├── export
    ├── import
    ├── snapshot
    ├── setup
    ├── demo
    ├── web
    └── migrate-journal
```
