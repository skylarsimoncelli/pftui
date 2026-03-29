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
| Medium-Timeframe Analyst | 75% | 85% | Mar 29 | ↓ (90→85 overall. `data quotes` alias #419 shipped to address command discoverability.) |
| Low-Timeframe Analyst | 85% | 90% | Mar 28 | → (stable. Alert triage #405 + regime transitions #407 + cross-timeframe resolve #410 shipped.) |
| High-Timeframe Analyst | 85% | 90% | Mar 26 | → (stable. Scenario suggest #366 shipped.) |
| Morning Intelligence | 75% | 85% | Mar 28 | ↑ (first scored → correlation break interpretation #412 addresses "clearer break data" request.) |
| Morning Brief | 85% | 80% | Mar 28 | → (stable. Morning-brief #363 shipped.) |
| Alert Investigator | 85% | 80-82% | Mar 25-26 | → (stable, consistent.) |
| Public Daily Report | 82% | 80% | Mar 28 | new (first scored review. Commodity coverage #402 shipped.) |
| Dev Agent | 92% | 94% | Mar 28 | → (stable high.) |

**Key changes since last review (Mar 29 run):**
- `data quotes` alias shipped (#419) — `pftui data quotes` now resolves to `data prices`. Cross-reference help text on both Prices and Futures commands.
- All explicit feedback items from all agents fully addressed.

**Shipped since last TODO update:**
1. ✅ **`data quotes` alias** — #419. Added `quotes` as clap alias for `data prices`. After_help cross-references on Prices and Futures. 3 new CLI tests (1911 total). Addresses medium-timeframe-analyst feedback (Mar 29 75/85): `data quotes` fails.

**Release eligibility:** 30 commits since v0.19.0 with 13 PRs, no P0 bugs, tests (1911) and clippy clean. **Eligible for v0.20.0** — substantial new work shipped.

**GitHub stars:** 7 — Homebrew Core requires 50+.
