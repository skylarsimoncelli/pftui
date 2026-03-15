# Investor Panel Skill

## Purpose
Run a multi-lens macro panel where multiple investor personas interpret the same pftui analytics payload and return structured positioning signals.

## Inputs
1. Fresh pftui cache state (`pftui refresh` recommended)
2. JSON payload from `collect-data.sh`
3. Persona markdown files in `personas/`
4. Output contract in `schema.json`

## Workflow
1. Run `./collect-data.sh` and store JSON output.
2. Load enabled personas from `config.toml`.
3. For each persona, pass:
- Persona markdown content
- Data payload JSON
- Required output schema (`schema.json`)
4. Parse each persona response as JSON and validate against `schema.json`.
5. Aggregate per-asset signals into consensus and divergence summaries.
6. Store panel summary with `pftui agent-msg send` and optionally publish externally.

## Suggested Orchestrator Prompt Frame
- System: "You are the investor described in this persona file. Stay faithful to that philosophy."
- User payload:
- Persona file content
- pftui JSON blob
- strict JSON schema
- Instruction: "Return only valid JSON matching schema."

## Consensus Rules
- Count bullish/bearish/neutral per tracked bucket (`cash`, `gold`, `btc`, `equities`, `oil`).
- Mark `strong_consensus` when >= 75% of personas agree.
- Mark `high_divergence` when bullish and bearish counts are within one vote.

## Operating Notes
- No trade execution. Output is analysis only.
- Use latest cached pftui state; panel quality depends on data freshness.
- Keep persona roster customizable through `config.toml` and `personas/custom/`.
