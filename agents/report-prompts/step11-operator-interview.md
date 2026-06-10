# Step 11 — Operator Interview + Decisions Pending

> Variables expected: `{DATE_ISO}`, `{OPERATOR_FOCUS}`.

The skill so far is a one-way pipe: agents read state, write content, deliver a PDF. Step 11 closes the loop. After the final summary in Step 10, **the orchestrator initiates a structured conversation with the operator** to capture their own reads, biases, and prospective moves — content nothing inside the analytical pipeline can produce on its own.

**Decisions Pending is also surfaced here, not in the PDF** — per operator: static decision cards in a PDF the operator cannot interact with are dead weight.

**Skip Step 11 when `--mode public`.**

# Order

1. **Decisions Pending walkthrough.** Load any Phase-4 architect cards + high-impact binary catalysts in next 14d. Present one at a time. Capture response immediately.
2. **Question sequence.** Open-ended, one at a time.

# Decisions Pending walkthrough

```bash
# Phase-4 architect cards (richest detail — evidence FOR / AGAINST / sizing math)
sqlite3 -json "$DB" "
  SELECT id, content FROM agent_messages
  WHERE from_agent = 'analyst-decisions'
    AND (category = 'decision-card' OR category = 'signal')
    AND date(created_at) = '{DATE_ISO}'
  ORDER BY id"

# High-impact binary catalysts in next 14d (pre-position questions)
pftui --cached-only analytics catalysts --json | jq -r '.items[] | select(.impact == "high")'
```

For each card, present to the operator:

```
**Decision card for <SYM>**

Question: <verbatim from card>

Evidence FOR:
- <list verbatim>

Evidence AGAINST:
- <list verbatim>

Recommendation: <from card>

Sizing math: <from card>

**Your call?** (yes / yes-if / no / wait / other / skip)
```

Capture immediately:

```bash
pftui agent message send \
  "Decision <SYM> {DATE_ISO}: <operator response verbatim>" \
  --from skylar --to synthesis --priority high \
  --category decision-response
```

Move to the next card. Once all are walked through, transition to the question sequence.

# Question sequence

Ask one at a time. Open-ended, not multiple choice. Let the operator type freely. After each answer:

1. Write the answer:

   ```bash
   pftui journal notes add \
     "[operator-interview-<topic-slug>]
   Q: <question>
   A: <verbatim answer>" \
     --section decisions --author skylar
   ```

2. If the answer contains a specific conviction shift, target, scenario probability, or position-change intent, mirror it into the structured tables:
   - Conviction → `pftui journal conviction set <SYM> <SCORE> "operator interview {DATE_ISO}: ..."`
   - Scenario → `pftui journal scenario update "<name>" --probability <X> --proposer skylar --evidence "operator stated view, interview {DATE_ISO}" --notes "operator interview ..."` (add `--override-conflict` if an analyst layer already moved this scenario today)
   - Position intent → `pftui agent message send "<plan>" --from skylar --to synthesis --priority high --category operator-intent`

The question sequence:

1. **Overall market read** — "What's your headline read on this week's tape? Anything the report missed or got wrong?"
2. **BTC** — "What's your conviction on BTC right now, and what would move you to add or trim from here?"
3. **Gold/Silver** — "Is the hard-money block still the highest-conviction asymmetric exposure on the board for you, or has this week shaken that?"
4. **Recent biases** — "Any bias you've been catching yourself in the last week or two? (e.g., anchoring on a level, fighting a tape, confirmation-seeking on a thesis.)"
5. **Specific moves on your radar** — "Anything you're actively considering executing in the next 1-2 weeks?"
6. **Targets and time horizons** — "Any price/yield/ratio targets you're watching for each held asset?"
7. **Scenarios not on the board** — "Anything you're tracking on the scenario tree that the report's scenario set doesn't cover?"
8. **Calibration check** — "Of the last week's predictions, which one are you most + least confident in? Why?"
9. **Free-form** — "Anything else you want logged for next week's run?"

If `{OPERATOR_FOCUS}` was substantive (e.g. "BTC + gold accumulation timing through 2026"), insert a focus-specific question after question 1:

> **Focus follow-up** — "On your accumulation plan: did the report's deep dive change anything about how you're thinking about entry pacing, sizing, or exit triggers?"

If the operator says `done`, `enough`, `skip rest`, or similar, end gracefully. Never blow past a `done`.

# After

Render a one-line confirmation:

```
✓ Captured N operator notes (author='skylar') · M conviction updates · K scenario updates · L decision-response messages. Next /pftui-report run will incorporate these via Phase-1 enrichment.
```
