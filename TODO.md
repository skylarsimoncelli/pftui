# TODO - pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.

---

## P2 - Coverage And Agent Consumption


### `pftui report build daily` — umbrella tracker (do not pick directly)
**Source:** Skylar (May 28). Depends on both `pftui report` scaffold and the chart-helper-port items above.
**Why:** Once chart rendering is native, the next layer up is the report ASSEMBLY — pulling data, ordering sections, inlining charts, and writing the markdown that feeds the PDF renderer. Today that work lives in the Claude `/pftui-report` skill orchestration plus the ad-hoc Python build script generated per run. Making it a native `pftui report build daily` command means: (1) anyone (not just Claude) can build a daily report from a populated DB; (2) the assembly logic gets `cargo test` coverage; (3) the Claude skill becomes much thinner — it spawns analysts, then calls `pftui report build daily`, then PRs the output. (4) Removes the Python build script entirely from the steady-state pipeline.
**Implementation plan:** All section TODOs and the assembler are landed. Remaining work is the skill migration below.
**Effort:** Complete except for the skill migration item.

### Migrate `/pftui-report` Claude skill — VALIDATION PENDING
**Source:** Skylar (May 28). Rewrite landed in this session (2026-06-03). `~/.claude/commands/pftui-report.md` shrunk from 1430 → 1025 lines: Step 4 now does only targeted web research; Step 5 is a single `pftui report build daily` invocation; the giant in-skill section template was retired. Privacy audit (Step 6), PDF render (Step 7a/b), website registry (Step 8), and PR/auto-merge (Step 9) unchanged. Assembler dry-run verified against the live DB: 11 public + 11 private sections fire in canonical order. `~/pftui-operator/charts.py` carries a DEPRECATED header (2026-06-03) noting the skill no longer imports it.
**Remaining validation:** Run `/pftui-report --mode both` end-to-end at least twice. Diff the produced markdown + PDFs against the prior Python-orchestrated outputs (allow byte-level whitespace/ordering diffs; flag content discrepancies as TODOs against the assembler, not the skill). Once validated, drop this entry and delete `~/pftui-operator/charts.py`.

---

## P3 - Long Term

---
