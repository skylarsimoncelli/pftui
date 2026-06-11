# CLI-DESIGN.md — the canonical CLI design doctrine

> **This is the conformance reference for the pftui CLI.** Every new command,
> flag, and output shape is implemented against this document. The CLI
> Perfection Program (TODO.md → P1) lands the conformance tests that make each
> section machine-enforced; until a section's test exists, this doc is the
> tie-breaker in review.
>
> Scoped 2026-06-11 from a full mechanical audit: 498 `--help` nodes walked,
> 403 leaf commands flag-fingerprinted, ~40 `--json` outputs shape-sampled
> against a synthetic fixture DB, every error path probed for exit code and
> stream discipline. Raw findings live in the audit notes within the
> CLI Perfection Program TODO briefs.

The CLI is pftui's most important interface: agents are the primary operators,
and every routine, report phase, and skill is a CLI consumer. The standard is
not "good enough" — it is that an agent can predict a command's path, flags,
output shape, and failure behavior **without reading the source**.

---

## 1. Domain map

### Canonical top-level domains

| Domain | Role |
|---|---|
| `agent` | Inter-agent workflows: message bus, debates |
| `analytics` | Derived analytical views (L2/L3 reads), alerts, views, epistemics |
| `data` | L0/L1 ingest: refresh, source reads, series status |
| `journal` | The operator/analyst write ledger: entries, notes, predictions, convictions, scenarios |
| `portfolio` | Holdings, value, targets, transactions, brokers |
| `report` | Report assembly and chart-rendering primitives |
| `research` | Research harness: signal registry, event studies, forecasts, shadow book |
| `system` | Config, diagnostics, schema, import/export, web/mobile servers |
| `console` | Interactive console (TTY-only; exempt from `--json` rules) |

This supersedes the stale "six domains" list in CLAUDE.md / docs/CLI-TREE.md —
`journal` and `research` are first-class and were always treated as such by
every routine. **The top-level `prediction` shortcut is NOT canonical** — it
violates the no-tree-bypass rule and is scheduled for removal (see §1.2).

### 1.1 One canonical path per noun

Every noun has exactly ONE canonical command path. A non-canonical path may
exist only as a **thin forward**: it must share the canonical path's clap
enum/struct (one definition — drift becomes a compile error) and dispatch
into the same function. Forward = same code path, never a parallel
implementation. The historical P1 (commit `e239b50`, "alias discipline fix")
happened because a duplicate clap surface let an alias bypass the canonical
write-discipline.

| Noun | Canonical path | Non-canonical paths today | Disposition |
|---|---|---|---|
| Macro scenarios (ledger) | `journal scenario …` | `analytics scenario …` | forward → shared enum (today: separate `AnalyticsScenarioCommand`, already drifted: `journal scenario update --id` missing on the analytics copy) |
| Predictions (analyst calls) | `journal prediction …` | `prediction …` (top-level), `analytics predictions add/scorecard/stats/unanswered`, `data predictions add/scorecard/stats/unanswered` | top-level `prediction` removed; grafted subcommands removed from `data`/`analytics predictions` |
| Prediction markets (Polymarket) | `data predictions …` (`markets`, `map`, `unmap`, `suggest-mappings`, `--geo`) | `analytics predictions markets/map/…` | forward or remove — distinct noun from analyst predictions; never re-merge the two |
| Convictions | `journal conviction …` | `analytics conviction …` | forward → shared enum (today: separate `AnalyticsConvictionCommand`) |
| Alerts (operator rules) | `analytics alerts …` | `data alerts …` | forward → shared enum (today: hand-mapped `DataAlertsRedirect`) |
| Capital flows | `data flows …` (L0 read) + `analytics flows summary` (derived) | — | legitimate layer split, not a duplicate |
| Event annotations | `analytics events …` | — | name collides with `research events` (signal events); both stay — different nouns, the help one-liners must disambiguate |

**Rule:** adding a second path to an existing noun requires a forward that
shares the clap type, plus a row in this table. The conformance test
(`tests/cli_canonical_paths.rs`, Program item C2) asserts every forward's
`--help` flag surface is byte-identical to the canonical path's.

### 1.2 No tree-bypass shortcuts

`pftui prediction …` (top-level, "shortcut for autoscore workflows") is the
only surviving bypass. It is removed by Program item C2; `journal prediction
auto-score` already runs in the `data refresh` tail, so nothing operational
needs the shortcut. No new shortcuts, ever — discoverability comes from the
tree, not from flat aliases.

---

## 2. Flag vocabulary matrix

Canonical flag names, with clap `alias`/`visible_alias` for back-compat.
New commands MUST use the canonical name; aliases exist only where a flag
already shipped under the old name.

| Canonical | Type / format | Meaning | Absorbs (as clap aliases) | Notes |
|---|---|---|---|---|
| `--symbol <SYM>` | string | one tradeable symbol / series key | `--asset` (25 commands), `--symbols` (csv where multi) | research/views/adversary families currently use `--asset`; `analytics technicals --symbols` stays plural-csv but gains `--symbol` for the single case |
| `--since <NNd\|NNw\|NNm\|YYYY-MM-DD>` | window token | start of a lookback window | `--days N` (15), `--window-days N` (4), `--window` (where it means lookback), `--period` (where it means lookback) | one parser (`parse_since`) shared by every command; bare-integer forms (`calibration-matrix rebuild --since 365`) normalize to `365d` |
| `--date <YYYY-MM-DD\|today\|yesterday>` | single day | as-of / anchor date | — | distinct from `--since`; never a window |
| `--author <name>` | vocab: author registry | writer identity (CLAUDE.md author table) | `--analyst` (views family), `--source-agent` (predictions), `--agent` (backtest/stats filters) | one identity vocabulary; `--layer` is NOT an identity |
| `--layer <low\|medium\|high\|macro\|macro-checkpoint>` | vocab enum | timeframe layer | `--timeframe` (predictions family) | casing canonical-lowercase everywhere (today `CANONICAL_LAYERS` exists twice with different casing/abbreviations) |
| `--from <agent>` / `--to <agent>` | author registry | message sender/recipient | — | RESERVED for the message bus. `analytics macro regime history/summary/transitions/confidence-trend --from <date>` is a collision → renamed `--since` with `--from` kept as a hidden alias |
| `--limit <N>` | int | max rows returned | — | already consistent (66 commands) |
| `--id <N>` | int | row id when it is a *filter among others* | — | when the id is the single required argument, it is positional (`remove <ID>`, `show <ID>`) — see §3 |
| `--json` | bool | structured output | — | on every non-interactive leaf (see §4/§7) |
| `--dry-run` | bool | preview without writing | — | every destructive/bulk write |
| `--confirm` | bool | non-interactive approval of a destructive plan | — | required in non-TTY contexts where a y/N prompt exists (see §5) |

The conformance test (`tests/flag_vocabulary.rs`, Program item C3) walks the
help tree and fails when a leaf exposes a non-canonical name without its
canonical twin, or mints a new synonym for any concept in this table.

---

## 3. Verb conventions

| Verb | Contract |
|---|---|
| `add` | insert a new row; fails or warns on duplicate — never silently upserts |
| `set` | UPSERT by natural key (`views set`, `target set`, `sources set`, `conviction set`) |
| `update` | mutate named fields of an existing row; **must fail loudly when the row does not exist** |
| `remove` | delete by id/key. **`delete` is non-canonical** (`analytics views delete`, `analytics risk-factors delete` gain `remove` with `delete` as alias) |
| `list` | collection read, filters as flags, newest-first unless documented |
| `show` | single-item read (positional key) or singleton payload |
| `score` / `record` | append-only ledger writes (L3); idempotent where documented |
| `refresh` | network fetch into L0/L1 |
| `rebuild` | deterministic L2 regeneration |

Identifier style: when a command operates on exactly one row and the id/key is
required, it is **positional** (`transaction remove 42`, `lessons revive 7`).
`--id` as a flag survives only where it is one optional filter among several.
Singular/plural: command groups are singular nouns (`journal prediction`,
`journal entry`); plural is reserved for collection-only groups
(`data predictions` markets, `analytics correlations`). Do not mint new
plural/singular twins.

**Write verbs must verify effect.** A mutation that matches zero rows exits
non-zero with a not-found error. (Audit finding: `journal prediction score
--id 999999 --outcome correct` prints `Scored prediction #999999 as correct`
and exits 0 — `db::user_predictions::score_prediction` ignores
`rows_affected == 0`. Program item C7 fixes the class.)

---

## 4. JSON output: the envelope

### 4.1 Audit baseline (why)

Sampled shapes today: bare arrays (`portfolio summary`, `portfolio drift`,
`transaction list`, `data news`, `analytics views list`, `recommendations
list`, `epistemics history`, `research expectancy`, …), `{count, <plural>}`
with a different plural key per command (`{count,predictions}`,
`{count,notes}`, `{count,messages}`, `{count,rules}`), keyless wrappers
(`{entries}`, `{convictions}`), `.items` (catalysts), and ad-hoc singletons.
Two routine docs disagree about `agent message list`'s shape (`.messages[]`
vs `.[]` — the latter is simply wrong). `portfolio performance --json` emits
**plain text** ("No portfolio snapshots found… Run `pftui refresh`" — also a
removed command path) when empty. Errors under `--json` emit free text on
stderr with no machine-readable object.

### 4.2 The envelope (end state)

Every `--json` response is a single JSON **object**:

```jsonc
{
  "data":     /* the payload: object or array — command-specific, documented */,
  "warnings": ["stale: cot_cache 17d past SLA"],   // always present, often []
  "meta": {
    "command": "analytics views list",             // canonical path, post-forwarding
    "schema_version": 1,                           // bump on breaking payload change
    "generated_at": "2026-06-11T15:30:00Z"
  }
}
```

On failure (any exit code ≠ 0 with `--json`): stdout still carries one object —

```jsonc
{
  "error": { "kind": "not-found", "message": "Transaction #999999 not found" },
  "meta": { "command": "portfolio transaction remove", "schema_version": 1 }
}
```

`error.kind` vocabulary (in `src/vocab.rs`): `usage`, `not-found`,
`validation`, `guard` (write-discipline rejections: evidence caps, conflicts,
confidence clamps), `io`, `network`, `conflict`.

Justification of `{data, warnings, meta}` over alternatives: bare arrays are
unextendable (adding a warning is a breaking change — the root cause of the
NOT-JSON-when-empty bug class); `{items}`/`{rows}` conventions still leave
warnings/versioning unsolved; reserving three keys gives every command the
same place for staleness/degradation warnings (EPISTEMICS "loud degradation")
without inventing per-command fields, and `meta.schema_version` makes payload
evolution detectable instead of silent.

### 4.3 Migration path (must not break the report pipeline)

Known JSON consumers (audited): `~/.claude/commands/pftui-report.md`
(`.positions[]?.symbol`, `.scored_count`, `.scored/.pending`,
`.rows_inserted`), `agents/routines/*.md` jq examples (`.messages[]`,
`.items[]`, `.positions[].symbol`, one broken `.[]`),
`agents/report-prompts/*.md`, `agents/investor-panel/collect-data.sh`
(currently calling pre-F42 removed paths — broken, silently nulling),
`scripts/parity_check.sh`, and the web/mobile servers (which consume Rust
structs directly, not CLI JSON — unaffected).

Phased, additive:

1. **Phase 1 (item C5)** — honesty fixes, no envelope: `--json` ALWAYS emits
   valid JSON (empty states included), errors emit the `error` object,
   bare-array commands KEEP their shape (frozen — no new bare arrays).
2. **Phase 2 (item C6)** — envelope opt-in: global `--envelope` flag (and
   `PFTUI_JSON_ENVELOPE=1`) wraps any `--json` output; legacy shape unchanged
   without it. Conformance test: every leaf's enveloped output parses with
   exactly the three reserved keys.
3. **Phase 3 (post-program, separate TODO)** — consumers migrated (the list
   above), envelope becomes the `--json` default, `--json-legacy` kept for
   one release. Never silently.

### 4.4 Token discipline

- Output is pretty-printed only when stdout is a TTY; piped/non-TTY output is
  compact single-line JSON (≈30-40% token saving for agents, no flag needed).
- Commands whose text output exceeds ~50 lines should offer `--brief`
  (one-line-per-item) where a routine demonstrably needs less; prefer compact
  defaults over flag proliferation.

---

## 5. TTY / prompt rules

- **Interactive prompts only when `stdin` is a TTY.** Non-TTY + missing
  required input → exit 2 with a usage error naming the flag
  (`error: missing --category <equity|crypto|forex|cash|commodity|fund>`).
  Audit: 4 prompt sites — `commands/add_tx.rs` (missing-field prompts),
  `commands/remove_tx.rs` (y/N confirm — fires even with `--json`),
  `config.rs` (first-launch wizard — fires on ANY command in a fresh env,
  including `system db-info --json`), `commands/setup.rs` (intentional).
- Destructive confirms accept `--confirm` to skip the prompt; non-TTY without
  `--confirm` is an error, never a hang and never a silent yes.
- The first-launch wizard runs only for `system setup` and TTY launches;
  any other command in an uninitialized environment proceeds with defaults
  and one stderr note.
- `system setup` is the only command allowed to be irreducibly interactive.

## 6. Exit codes + stream discipline

| Code | Meaning |
|---|---|
| 0 | success (including legitimately-empty results) |
| 1 | runtime failure (not-found, validation, guard, io, network — `error.kind` distinguishes) |
| 2 | usage error (clap; also non-TTY missing-input per §5) |

- **stdout is payload only**: the JSON document or the human-readable table.
  Prompts, notes, progress, `[timing]`, deprecation warnings → stderr.
- In `--json` mode, warnings agents must see go INTO the JSON `warnings`
  array (stderr is a courtesy copy, not the contract).
- "Success" text for a write names the effect and the id
  (`Scored prediction #42 as correct`); zero-effect writes are errors (§3).

## 7. `--json` coverage

Every leaf command supports `--json` except an explicit exemption registry
(asserted by the conformance test, Program item C8):
`console`, `system setup`, `system demo`, `system snapshot`, `system web`,
`system mobile serve` (servers/interactive), `system export`/`system import`
(own format contract). Currently missing without exemption: `portfolio
history` (TODO filed), `portfolio target set/remove`, `portfolio watchlist
add/remove`, `analytics alerts add/remove/rearm/seed-defaults`,
`system mirror sync`, `system mobile enable/disable/token generate`.

## 8. Enum vocabularies (`src/vocab.rs`)

One module owns every cross-command vocabulary: canonical Rust enums with
`Display`/`FromStr`/serde and clap `ValueEnum`, consumed by **writers AND
readers** (the decision-card P1 was a writer/reader vocabulary split). No
string-literal `matches!` lists outside the module.

| Vocabulary | Values (canonical) | Today scattered across |
|---|---|---|
| Direction | `bullish`, `bearish`, `neutral` | 20+ files |
| Layer | `low`, `medium`, `high`, `macro` (+ `macro-checkpoint` for predictions; `cross` for messages) | two divergent `CANONICAL_LAYERS` consts (`views_stale.rs` lowercase, `conviction_trajectory.rs` `LOW/MED/HIGH/MACRO`) |
| Author registry | CLAUDE.md author table (`skylar`, `analyst-*`, `agent-feedback`, `system`, plus measurement layers `blind`, `antithesis`) | prompt docs + ad-hoc strings |
| Message category | `signal`, `feedback`, `alert`, `handoff`, `escalation`, `decision-card`, `macro-checkpoint-reeval` | `commands/agent_msg.rs` validator vs report loader vs scorer emitter |
| Recommendation action | `add`, `wait`, `hold`, `trim`, `avoid` | recommendations + shadow book + decision cards |
| Conviction band | `low`, `medium`, `high` | predictions, calibration, preflight |
| Tx category | `equity`, `crypto`, `forex`, `cash`, `commodity`, `fund` | add_tx prompt + models |
| Outcome | `correct`, `partial`, `wrong` | scorers, stats, lessons |
| Decision response tokens | `yes`, `yes-if`, `no`, `wait`, `other` | decision cards, operator replies |
| Urgency / priority | `high`, `normal`, `low` | messages, decision cards |
| `error.kind` | §4.2 list | new |

Conformance: `tests/vocab_conformance.rs` greps `src/**/*.rs` (tests excluded)
for known vocabulary literals appearing in `matches!`/array-literal validation
position outside `vocab.rs` and fails with a pointer here.

## 9. Doc surfaces bound to this doctrine

Whenever the CLI surface changes, these must change in the same PR (the
doc-drift tests in DATA-ARCHITECTURE.md §Doc-drift enforcement catch literal
command rot): `AGENTS.md` (CLI reference tables), `README.md`,
`docs/CLI-TREE.md` (must match `--help`; Program item C9 makes it
generated), `docs/CLI-MIGRATION.md` (one row per removed/forwarded path),
`agents/routines/*.md`, `agents/report-prompts/*.md`, and — orchestrator-side,
out of repo — `~/.claude/commands/pftui-report.md`.
