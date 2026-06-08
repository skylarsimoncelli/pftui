# `agents/report-prompts/` — subagent prompt templates for `/pftui-report`

This directory holds the canonical prompt templates the `/pftui-report` skill (`~/.claude/commands/pftui-report.md`) spawns subagents with. Each file is a single-purpose template with `{PLACEHOLDER}` variables the skill substitutes at run time.

Pulling these out of the skill file delivers four wins:

1. **Iteration.** Editing a focused 60-line prompt vs. scrolling a 1700-line skill.
2. **Version control.** The skill file lives at `~/.claude/commands/` and isn't in git; these templates *are* in git, so changes are diffable, reviewable, and revertable.
3. **Reuse.** Cron jobs, evening-analysis, alert-watchdog, and any future on-demand agent can use the same templates without duplicating the wording.
4. **Auditability.** Anyone reviewing what an analyst was actually asked can read one file instead of reconstructing a substitution from skill prose.

## Convention

- Files are markdown, named `<phase>-<purpose>.md` so listing the directory in lexicographic order walks the pipeline.
- Templates contain `{PLACEHOLDER}` variables in curly braces (no whitespace inside).
- Templates that share an opening block (e.g. the Phase-1 enrichment context, the operator-focus injection) reference a shared snippet by `{INCLUDE _shared-<name>}`. The skill expands the include first, then substitutes variables.
- The skill is the only authority that knows which variables exist; templates document the variables they expect at the top.

## Variable reference

Every variable the skill can provide. Templates declare what they need at the top.

| Variable | Source | Shape |
|---|---|---|
| `{OPERATOR_FOCUS}` | Step 0d capture | Multi-line free text — the operator's prompt verbatim. |
| `{HELD_ASSETS}` | Step 3 held-asset query | Newline-separated symbol list. |
| `{DATE_ISO}` | Step 0 | `YYYY-MM-DD`. |
| `{SKYLAR_JOURNAL_7D}` | Step 2d | Recent operator-authored journal entries. |
| `{LAYER_OWN_HISTORY}` | Step 2d (per-layer) | This layer's 30d view history per held asset. |
| `{LAYER_DIVERGENCE_DIGEST}` | Step 2d (per-layer) | Where this layer disagreed with the other three. |
| `{MACRO_TAPE_7D}` | Step 2d | 7d delta block (CPI/PCE/yields/DXY/VIX/SPX/gold/oil). |
| `{INBOX_FROM_AGENTS}` | Step 2d | Unread agent_messages for the operator. |
| `{LESSON_BOOK}` | Step 2c | Trailing-25 prediction_lessons rendered as bullets. |
| `{MANDATORY_CONTEXT}` | Step 2c-mandatory | Skylar's analytical framework + first principles. |
| `{LAYER}` | Per-call | One of `low` / `medium` / `high` / `macro`. |
| `{ASSET}` | Per-call | A single symbol when the template is per-asset. |
| `{PERSONA_PATH}` | Step 3.7b panel iteration | Path to a persona file under `~/pftui/agents/investor-panel/personas/`. |
| `{CANDIDATE_MD}` | Step 5 output | Path to the assembler's candidate markdown. |
| `{SECTION_CATALOG}` | Skill | Inline section catalog the composition pass consults. |

## Adding a template

1. Create `<phase>-<purpose>.md` with a header documenting the variables it expects.
2. Reference any shared blocks via `{INCLUDE _shared-<name>}`.
3. Update the skill's Step that spawns this agent to load the template and substitute the variables.
4. Run the smoke test (`cargo test --test report_prompt_templates`) to confirm every placeholder the template uses is in the skill's known list.

## Files in this directory

| File | Phase | Purpose |
|---|---|---|
| `_shared-operator-focus.md` | shared | The "operator focus" injection block — included in every analyst template. |
| `phase1-timeframe-analyst.md` | 1 | LOW / MEDIUM / HIGH / MACRO analyst template (parameterised by `{LAYER}`). |
| `phase2a-adversary.md` | 2a | Synthesis-time adversary contesting the convergence. |
| `phase2b-panel-persona.md` | 2b | Investor-panel persona (parameterised by `{PERSONA_PATH}`). |
| `phase2c-external-research.md` | 2c | External TA + research agent (web-search outside pftui's news pipeline, compare per-asset to our convergence). |
| `phase3-synthesis-writer.md` | 3 | Per-asset bull/bear/change-mind/RR + Week-in-Review economy + Macro & News Outlook + Closing notes. |
| `phase3b-deep-dive.md` | 3b | NEW — long-form narrative tailored to `{OPERATOR_FOCUS}`. |
| `phase4-decision-architect.md` | 4 | Portfolio decision cards from drift + convergence + catalysts. |
| `phase5-steelman-bull.md` | 5 | Per-asset steelman bull. |
| `phase5-steelman-bear.md` | 5 | Per-asset steelman bear. |
| `phase5.5-composition.md` | 5.5 | Orchestrator's prompt-to-self for the composition pass. |
| `phase6-debate-moderator.md` | 6 | Final debate roundup. |
| `step11-operator-interview.md` | 11 | Operator interview question script (post-PDF). |
