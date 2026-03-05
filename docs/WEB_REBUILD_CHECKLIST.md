# Web Rebuild Master Checklist

Last updated: 2026-03-05
Purpose: authoritative implementation tracker for all web parity sessions.

## Phase Completion Summary
- Phase 0: `completed` (all P0-* tasks `done`)
- Phase 1: `completed` (all P1-* tasks `done`)
- Phase 2: `completed` (all P2-* tasks `done`)
- Phase 3: `completed` (all P3-* tasks `done`)
- Phase 4: `completed` (all P4-* tasks `done`)
- Phase 5: `completed` (all P5-* tasks `done`)
- Phase 6: `completed` (all P6-* tasks `done`)
- Phase 7: `completed` (all P7-* tasks `done`)
- Phase 8: `completed` (all P8-* tasks `done`)
- Phase 9: `completed` (all P9-* tasks `done`)
- Phase 10: `completed` (all P10-* tasks `done`)
- Phase 11: `completed` (all P11-* tasks `done`)
- Verification snapshot (2026-03-05):
  - `/Users/skylar/.cargo/bin/cargo test web::view_model -- --nocapture` => pass
  - `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` => pass
  - `npx playwright test tests/web.integration.spec.ts` => pass
  - `/Users/skylar/.cargo/bin/cargo run -- web --bind 127.0.0.1 --port 18080 --no-auth` => pass (smoke check: background loops boot + refresh starts)

## Status Values
- `todo`
- `in_progress`
- `done`
- `blocked`

## Session Update Protocol
- Every session must update all touched task rows before handing off.
- Every session must append an entry to the `Change Log` section.
- A task may move to `done` only with explicit validation evidence in the `Validation` column.
- If a task changes API shape, update `docs/WEB_API_SCHEMA_v1.md` in the same session.

## Master Tasks
| Phase | Task ID | Task | Status | Owner Session | Dependencies | Definition of Done | Validation | Changed interfaces/docs |
|---|---|---|---|---|---|---|---|---|
| 0 | P0-01 | Create authoritative parity matrix | done | session-phase0-1 | none | Matrix covers all major tabs/workflows with severity+owner phase | doc review | `WEB_PARITY_MATRIX.md` |
| 0 | P0-02 | Freeze naming contracts (tabs, keys, status vocabulary) | done | session-phase0-1 | P0-01 | Contract freeze table exists and is used as reference | doc review | `WEB_PARITY_MATRIX.md` |
| 0 | P0-03 | Add endpoint status markers to API schema | done | session-phase0-1 | P0-01 | Schema explicitly marks `implemented/planned` | schema review | `WEB_API_SCHEMA_v1.md` |
| 1 | P1-01 | Replace hardcoded web markets universe with shared contract source | done | session-phase0-1 | P0-02 | `GET /macro` indicators derive from shared market contract | `/Users/skylar/.cargo/bin/cargo test web::view_model -- --nocapture` | `src/web/view_model.rs`, `src/web/api.rs` |
| 1 | P1-02 | Replace hardcoded web economy sections with shared contract source | done | session-phase0-1 | P0-02 | `GET /macro` sections derive from shared economy contract and order | `/Users/skylar/.cargo/bin/cargo test web::view_model -- --nocapture` | `src/web/view_model.rs`, `src/web/api.rs` |
| 1 | P1-03 | Normalize watchlist quote symbol semantics with TUI | done | session-phase0-1 | P0-02 | Watchlist quote + day change uses shared normalization | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` | `src/web/view_model.rs`, `src/web/api.rs` |
| 1 | P1-04 | Add contract tests for shared market/economy adapters | done | session-phase0-1 | P1-01,P1-02 | Tests assert ordering/uniqueness and baseline symbols | `/Users/skylar/.cargo/bin/cargo test web::view_model -- --nocapture` | `src/web/view_model.rs` |
| 2 | P2-01 | Add web background refresh loop (prices + macro symbol set) | done | session-phase2-1 | P1-01 | `pftui web` refreshes caches without separate `pftui refresh` | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `/Users/skylar/.cargo/bin/cargo run -- web --bind 127.0.0.1 --port 18080 --no-auth` | `src/web/server.rs`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 2 | P2-02 | Add web RSS ingest loop + cleanup | done | session-phase2-1 | P2-01 | News cache populated/cleaned during web runtime | `/Users/skylar/.cargo/bin/cargo test web::server -- --nocapture` + `/Users/skylar/.cargo/bin/cargo run -- web --bind 127.0.0.1 --port 18080 --no-auth` | `src/web/server.rs`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 3 | P3-01 | Implement global asset search API (beyond loaded rows) | done | session-phase3-1 | P1-01 | Search returns global asset universe matches | `/Users/skylar/.cargo/bin/cargo test web::api -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 3 | P3-02 | Add star/unstar watchlist mutation from search overlay | done | session-phase3-1 | P3-01,P1-03 | Search detail can add/remove watchlist | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 3 | P3-03 | Enrich asset popup dataset (chart + market data) | done | session-phase3-1 | P3-01 | Overlay contains chart and key data blocks | `npx playwright test tests/web.integration.spec.ts` + `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` | `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 4 | P4-01 | Move positions right pane to asset overview only | done | session-phase4-1 | P0-01 | No market overview card on positions | `npx playwright test tests/web.integration.spec.ts` | `src/web/static/index.html`, `tests/web.integration.spec.ts`, `docs/WEB_PARITY_MATRIX.md` |
| 4 | P4-02 | Move market overview to Markets tab only | done | session-phase4-1 | P4-01 | Market overview consolidated in markets | `npx playwright test tests/web.integration.spec.ts` | `src/web/static/index.html`, `tests/web.integration.spec.ts`, `docs/WEB_PARITY_MATRIX.md` |
| 5 | P5-01 | Add alert create endpoint + UI form | done | session-phase5-1 | P0-03 | Users can create alert rules in web | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 5 | P5-02 | Add alert remove endpoint + UI action | done | session-phase5-1 | P5-01 | Users can remove alerts | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 5 | P5-03 | Add ack/rearm alert lifecycle endpoints + actions | done | session-phase5-1 | P5-01 | Triggered alerts can be acked/rearmed | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 6 | P6-01 | Add journal create endpoint + UI | done | session-phase6-1 | P0-03 | Journal entry creation works | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 6 | P6-02 | Add journal update endpoint + UI editing | done | session-phase6-1 | P6-01 | Journal content/status editable | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 6 | P6-03 | Add journal delete endpoint + UI | done | session-phase6-1 | P6-01 | Journal deletion works | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 7 | P7-01 | Add transaction create endpoint + UI | done | session-phase7-1 | P0-03 | Transaction creation works in full mode | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/db/transactions.rs`, `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 7 | P7-02 | Add transaction update support (DB + API + UI) | done | session-phase7-1 | P7-01 | Transaction edit works | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/db/transactions.rs`, `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 7 | P7-03 | Add transaction delete endpoint + UI | done | session-phase7-1 | P7-01 | Transaction deletion works | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/db/transactions.rs`, `src/web/api.rs`, `src/web/server.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 8 | P8-01 | Expand markets tab density (movers, richer metrics) | done | session-phase8-1 | P2-01 | Markets tab no longer sparse | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/api.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 8 | P8-02 | Expand economy tab depth (macro, calendar, sentiment) | done | session-phase8-1 | P2-01 | Economy tab presents available macro datasets | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/api.rs`, `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_API_SCHEMA_v1.md`, `docs/WEB_PARITY_MATRIX.md` |
| 9 | P9-01 | Build timeline-first news UI | done | session-phase9-1 | P2-02 | News renders chronological timeline with filters | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/static/index.html`, `tests/web.mocks.ts`, `tests/web.integration.spec.ts`, `docs/WEB_PARITY_MATRIX.md` |
| 10 | P10-01 | Complete keyboard navigation parity hardening | done | session-phase10-1 | P3-03,P4-02 | Web key behavior aligned to frozen key intents | `/Users/skylar/.cargo/bin/cargo test web:: -- --nocapture` + `npx playwright test tests/web.integration.spec.ts` | `src/web/static/index.html`, `tests/web.integration.spec.ts`, `docs/WEB_PARITY_MATRIX.md` |
| 11 | P11-01 | Add API contract tests for all new mutating endpoints | done | session-phase11-1 | P5-03,P6-03,P7-03 | Contract coverage complete | `/Users/skylar/.cargo/bin/cargo test web::api::tests:: -- --nocapture` | `src/web/api.rs`, `docs/WEB_PARITY_MATRIX.md` |
| 11 | P11-02 | Add E2E flows for CRUD/search/navigation parity | done | session-phase11-1 | P11-01 | Playwright covers core battlestation flows | `npx playwright test tests/web.integration.spec.ts` | `tests/web.integration.spec.ts`, `docs/WEB_PARITY_MATRIX.md` |

## Phase Gates
- Gate A (start phase >=2): P0-* and P1-* all `done`.
- Gate B (start phase >=5): P2-* and P3-* at least in `in_progress` with stable API docs.
- Gate C (release candidate): P4..P11 core tasks complete and parity matrix P0/P1 rows are `implemented`.

## Change Log
- 2026-03-05 — `session-phase0-1`: Added master checklist and seeded tasks/phases/dependencies/DoD for multi-session execution.
- 2026-03-05 — `session-phase0-1`: Marked phase-level completion explicitly and attached passing test command evidence for Phase 1 tasks.
- 2026-03-05 — `session-phase2-1`: Implemented web background loops for price/macro refresh and RSS ingest/cleanup; validated via web test suite and runtime smoke startup logs.
- 2026-03-05 — `session-phase3-1`: Implemented API-backed global search, search-driven watchlist star/unstar mutations, and enriched asset detail payload/overlay; validated with Rust web tests and Playwright integration flow.
- 2026-03-05 — `session-phase4-1`: Removed positions-tab market overview, consolidated market overview to Markets tab, and made positions right-pane header asset-specific; validated with Playwright integration coverage.
- 2026-03-05 — `session-phase5-1`: Implemented alert create/remove/ack/rearm API endpoints and web actions (alerts tab + overlay), with lifecycle state transitions validated via Rust web tests and Playwright integration flow.
- 2026-03-05 — `session-phase6-1`: Implemented journal create/update/delete API endpoints and Journal tab create/edit/delete controls; validated via Rust web tests and Playwright integration flow.
- 2026-03-05 — `session-phase7-1`: Implemented transaction create/update/delete across DB + API + Transactions tab UI form/actions; validated with Rust web tests and Playwright integration flow.
- 2026-03-05 — `session-phase8-1`: Implemented Markets density upgrades (macro-sourced movers + breadth stats) and Economy depth snapshot cards (BLS/sentiment/calendar/predictions) via `/api/macro`, with web UI rendering and Playwright integration assertions.
- 2026-03-05 — `session-phase9-1`: Implemented timeline-first News tab UX with grouped chronological rendering and source/category/hours/search filters backed by `/api/news` query parameters; validated via web tests and Playwright integration flow.
- 2026-03-05 — `session-phase10-1`: Hardened keyboard parity for shared intents (`1..8` view switching, `w` watchlist jump, `/`, `j/k`, `Enter`, `Esc`, `t`) with input-safe shortcut handling and Playwright keyboard flow coverage.
- 2026-03-05 — `session-phase11-1`: Added API contract tests for mutating watchlist/alerts/journal/transaction handlers and added an end-to-end battlestation parity flow (search, watchlist, alerts, journal, transactions, news, keyboard navigation) in Playwright.
