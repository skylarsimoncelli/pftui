# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P2 - Coverage And Agent Consumption


### `pftui report build daily` — umbrella tracker (do not pick directly)
**Source:** Skylar (May 28). Depends on both `pftui report` scaffold and the chart-helper-port items above.
**Why:** Once chart rendering is native, the next layer up is the report ASSEMBLY — pulling data, ordering sections, inlining charts, and writing the markdown that feeds the PDF renderer. Today that work lives in the Claude `/pftui-report` skill orchestration plus the ad-hoc Python build script generated per run. Making it a native `pftui report build daily` command means: (1) anyone (not just Claude) can build a daily report from a populated DB; (2) the assembly logic gets `cargo test` coverage; (3) the Claude skill becomes much thinner — it spawns analysts, then calls `pftui report build daily`, then PRs the output. (4) Removes the Python build script entirely from the steady-state pipeline.
**Implementation plan:** All section TODOs and the assembler are landed. Remaining work is the skill migration below.
**Effort:** Complete except for the skill migration item.

### Migrate `/pftui-report` Claude skill to use native `pftui report` commands
**Source:** Skylar (May 28). Depends on `pftui report build daily` (above) being landed.
**Why:** Now that `pftui report build daily` exists end-to-end, the Claude skill at `~/.claude/skills/pftui-report.md` can be substantially simplified: no ad-hoc Python build script per run, no per-step data-gathering bash blocks that prepare chart inputs. The skill's responsibilities shrink to: Step 0 health collection + blocker fixes, Step 1 data refresh, Step 3 spawning the four analyst subagents, then calling `pftui report build daily --mode <m>`, then the privacy audit / PDF render / website registry / PR steps.
**Scope:** (1) Rewrite the relevant sections of `~/.claude/skills/pftui-report.md` (Step 2 CLI bundle, Step 2b deep bundle, Step 2c thesis/lessons fetch, Step 4 synthesis, Step 5a public markdown, Step 5b private markdown) to call `pftui report build daily` instead of doing data collection + assembly in skill bash + Python. The bundles can still be staged for the analysts (they need them as input), but the synthesis-and-write step becomes a single CLI call. (2) Decommission `~/pftui-operator/charts.py` once all charts are ported and used by zero remaining code paths — leave the file but mark it deprecated in a header comment and remove the skill's `sys.path.insert` line. (3) Update the skill's failure-modes section: `pftui report build daily` errors should be diagnosed by reading the command's stderr; the skill's responsibility is to surface those errors, not to debug section assembly. (4) Run `/pftui-report` end-to-end at least twice on the new code path before considering this item done; compare the resulting markdown + PDFs against the prior Python-orchestrated outputs and confirm parity. Files: `~/.claude/skills/pftui-report.md` (substantial rewrite), `~/pftui-operator/charts.py` (deprecation header). Tests: not applicable in pftui (skill-side change); verification is the parity comparison.
**Effort:** 4–7 days (mostly skill testing + iteration).

---

## P3 - Long Term

### [Claude-WIP 2026-06-02l — DO NOT PICK] F59: Capital Flow Tracking
**Source:** Competitive research (NOFX institutional flow data).
**Why:** Institutional fund flows, ETF creation/redemption, and open interest changes reveal positioning that price alone doesn't show.
**Scope:** New `data flows` source pulling ETF flow data (ETF.com or similar), institutional 13F filings, and crypto exchange flow data. New table `capital_flows`. Integration into agent routines.
**Effort:** 3–4 weeks.


---
