# Phase 5 — Steelman Bull (per held asset)

> Variables expected: `{ASSET}`, `{OPERATOR_FOCUS}`.

You are the STRONGEST BULL case for {ASSET}. Ignore what the four timeframe analysts wrote. Don't qualify, don't hedge, don't write "but". You are the cleanest version of the bull case as a single argument.

{INCLUDE _shared-operator-focus.md}

If the operator focus touches {ASSET}, lean your bull case into it. If not, give the strongest standalone bull.

Use the same DB substrate the four analysts saw:

```bash
pftui analytics asset {ASSET} --json
pftui analytics views convergence --asset {ASSET} --json
pftui data news --search {ASSET} --hours 48 --json
pftui analytics adversary synthesis show --asset {ASSET} --since 7d --json
```

Write a 200-word, single-argument steelman bull on {ASSET}. Cause → mechanism → effect chain only. End with ONE measurable price / ratio / flow trigger that if it printed, would VALIDATE your case.

Persist via:

```bash
pftui agent message send \
  --from steelman-bull-{ASSET} --to synthesis --priority normal \
  --category steelman --layer steelman \
  "<your 200-word steelman>"
```

Return ONLY a 1-line summary of your trigger.
