# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P0 - Critical

_(none)_

## P1 - Data Quality & Agent Reliability

_(none)_

## P2 - Coverage And Agent Consumption

_(none)_

## P3 - Long Term

_(none)_

---

## Feedback Summary

**Latest scores per tester (most recent scored review):**

| Tester | Usefulness | Overall | Date | Trend |
|--------|-----------|---------|------|-------|
| Evening Analyst | 78% | 75% | Mar 28 | ↑ (72→78 usefulness, 75→75 overall. --claim fix #392 + cross-timeframe #396 + alerts redirect #398 shipped. **Lowest scorer — priority.**) |
| Medium-Timeframe Analyst | 85% | 90% | Mar 28 | ↑ (88→90 overall. Scenario impact-matrix #387 + regime transitions #407 shipped.) |
| Low-Timeframe Analyst | 85% | 90% | Mar 28 | → (stable. Alert triage #405 + regime transitions #407 + cross-timeframe resolve #410 shipped.) |
| High-Timeframe Analyst | 85% | 90% | Mar 26 | → (stable. Scenario suggest #366 shipped.) |
| Morning Intelligence | 75% | 85% | Mar 28 | ↑ (first scored → correlation break interpretation #412 addresses "clearer break data" request.) |
| Morning Brief | 85% | 80% | Mar 28 | → (stable. Morning-brief #363 shipped.) |
| Alert Investigator | 85% | 80-82% | Mar 25-26 | → (stable, consistent.) |
| Public Daily Report | 82% | 80% | Mar 28 | new (first scored review. Commodity coverage #402 shipped.) |
| Dev Agent | 92% | 94% | Mar 28 | → (stable high.) |

**Key changes since last review (Mar 28 prior run):**
- CLI description coverage shipped (#414) — 88 undocumented subcommands now have descriptions in `--help` and `system search`. Zero empty descriptions across all namespaces.
- All explicit feedback items from all agents fully addressed.

**Shipped since last TODO update:**
1. ✅ **CLI subcommand descriptions** — #414. 88 doc comments added across journal (28) and analytics (60) enums. Every command now self-documenting in `system search --json`. Addresses evening-analyst discoverability pattern (lowest scorer 78/75).

**Release eligibility:** 28 commits since v0.19.0 with 11 PRs, no P0 bugs, tests (1896) and clippy clean. **Eligible for v0.20.0** — substantial new work shipped.

**GitHub stars:** 7 — Homebrew Core requires 50+.
