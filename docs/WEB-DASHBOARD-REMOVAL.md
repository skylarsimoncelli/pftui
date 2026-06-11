# Web Dashboard Removal — Complete Inventory & Implementation Checklist

**Status:** scoped 2026-06-11 — abandonment decided by the operator. This document is the
implementing agent's checklist. TODO.md carries the capability briefs (`### Web Dashboard Removal`, P2).

**Scope decision:** the web DASHBOARD (`pftui system web`, `src/web/`) is deleted.
**Explicitly OUT of scope — do not touch:**

- `website/` — that is **pftui.com**, the public reports site the `/pftui-report` skill publishes
  PDFs to. Completely separate concern. `WEBSITE_DEPLOYMENT.md` and `.github/workflows/website.yml`
  stay exactly as they are. Any change under `website/` in the removal PR is a scoping error.
- `src/mobile/` — the **native iOS/macOS API server is NOT part of the dashboard and is KEPT.**
  Verified independent and in active deployment:
  - separate CLI tree (`pftui system mobile enable|disable|status|token|serve`), separate TLS+token
    auth (`src/mobile/auth.rs`, argon2-hashed tokens in config), own port (9443)
  - real native Swift clients in `mobile/app/PftuiMobile.xcodeproj` (iOS + macOS targets),
    built by `release.yml` job `mobile-ios`
  - deployed as a systemd service: `deploy/systemd/pftui-mobile.service`; `scripts/deploy.sh`
    restarts `pftui-daemon pftui-mobile`; `agents/routines/dev-agent.md:119-125` health-checks it
  - the only coupling to the dashboard is `use crate::web::view_model` in `src/mobile/server.rs`
    — resolved by relocating `view_model.rs` (see Code section)
- `pftui data dashboard …` (macro/global data dashboards) — CLI analytics views, nothing to do
  with the web dashboard. The word "dashboard" in those commands stays.

---

## 1. Code (Rust)

### 1.1 Relocate FIRST (shared, not web-only)

| Item | Detail |
|---|---|
| `src/web/view_model.rs` (380 lines, incl. its `mod tests`) | Used by `src/mobile/server.rs:32` and `src/analytics/situation.rs:15` (`market_overview_symbols()` at situation.rs:381). Move to `src/analytics/view_model.rs` (or `src/models/`), update both `use crate::web::view_model` imports + `src/web/mod.rs` re-export removal. Do this in the same commit as the deletion so nothing transiently breaks. |

### 1.2 Delete

| Path | Notes |
|---|---|
| `src/web/mod.rs` (6 lines) | module exports |
| `src/web/server.rs` (358 lines) | axum router, SSE stream (`/stream`), embedded frontend via `include_str!("static/index.html")`, inline `mod tests` |
| `src/web/api.rs` (3111 lines) | ~30+ REST handlers incl. write endpoints (POST/PATCH/DELETE on watchlist, transactions, alerts, journal), inline `mod tests` at :2631 — **these are the 6 flaky `web::api::tests::*` tests** (documented in CHANGELOG ~line 150: fail under high test parallelism from SQLite shared-memory contention, pass in isolation). Deletion removes a known flake source from the suite. |
| `src/web/auth.rs` (664 lines) | bearer-token middleware, inline tests |
| `src/web/static/index.html` (2412 lines, ~113 KB) | embedded SPA frontend — binary shrinks by this plus the handler code |
| `src/main.rs:19` | `mod web;` |
| `src/main.rs:1237-1241` | the `SystemCommand::Web { .. }` arm of the `should_sync_mirror_on_startup` guard (KEEP the adjacent `MobileCommand::Serve` arm at 1243-1247) |
| `src/main.rs:1715-1731` | `cli::SystemCommand::Web { … } => web::run_server(…)` dispatch (KEEP the `Mobile` dispatch right below) |
| `src/cli.rs:2115-2128` | `SystemCommand::Web { port, bind, no_auth }` variant + doc comment "Start the web dashboard server" (KEEP `Mobile` at 2129+) |

### 1.3 Config (`src/config.rs`)

- **No web-server config keys exist** — `system web` is configured purely by CLI flags; the auth
  token is generated per-run and never persisted. Nothing to migrate.
- KEEP all of `MobileServerConfig` / `MobileApiToken` / `MobileTokenPermission` (mobile stays).
- `home_tab` is a shared TUI preference (config.rs:389) — KEEP the key; the web `/preferences/home-tab`
  GET/PUT handlers die with `api.rs`. Reword the comment at config.rs:387
  ("Preferred home tab when opening TUI/Web." → "…opening the TUI.").

### 1.4 Cargo.toml dependencies (sole-use verified by grep over `src/` + `tests/`)

| Dep | Users | Action |
|---|---|---|
| `tokio-stream` (line 51) | `src/web/server.rs` ONLY | **remove** |
| `tokio-util` (line 50) | **no `use`/path reference anywhere in src/ or tests/** — already dead | **remove** (confirm with `cargo build`) |
| `axum` (44) | web + mobile | keep (mobile) |
| `axum-server` (45) | mobile only | keep |
| `tower` (46) | web + mobile | keep (mobile) |
| `tower-http` (47) | web + mobile | keep (mobile) |
| `rustls` (61), `rustls-pemfile` (62) | mobile only | keep |
| `argon2`, `tracing`, `tracing-subscriber` | mobile only (sha2 also brokers/report) | keep |
| `tokio`, `reqwest`, `sqlx` | system-wide | keep |

After removal run `cargo build` — if `tower`'s `limit` feature or `tower-http`'s
`cors`/`fs`/`timeout` features were web-only, trim features to what `src/mobile/server.rs`
actually uses (it uses `trace`); do not guess, let the compiler confirm.

## 2. Tests & CI

| Item | Action |
|---|---|
| Inline `mod tests` in `src/web/{server,auth,api,view_model}.rs` | die with the module (view_model's tests move with the file). Removes the 6 flaky `web::api::tests::*` (see 1.2). |
| `tests/web.integration.spec.ts`, `tests/web.visual.spec.ts`, `tests/web.mocks.ts` | delete (Playwright suites against the dashboard) |
| `playwright.config.ts`, `package.json` (root — named `pftui-web-tests`, only web-test scripts), `package-lock.json` | delete all three; nothing else consumes them (`website/` deploy is a plain file copy, no npm) |
| `.github/workflows/ci.yml` — `web-tests` job (lines ~54-90: Node setup, `npm ci`, Playwright install/run, `web-visual-snapshots` + `playwright-report` artifact uploads) | delete the whole job |
| `.github/workflows/release.yml` — "Web parity checklist gate" step (lines ~24-27, `web-stable-*` tag gate) and the Playwright install/run steps (~34-35) | delete; KEEP the `mobile-ios` job (~100-128) and everything website-related (~255+) |
| `scripts/check_web_parity_checklist.sh` | delete (only caller is the release gate above) |
| `tests/cli_help_smoke.rs` | no edit needed — walks the live CLI tree, adapts automatically |
| `tests/doc_commands.rs` | covers README.md + AGENTS.md only; passes once the doc sweep removes `system web` examples |
| `tests/fixtures/flows/` | UNRELATED (ETF flows data for `src/data/flows.rs`) — keep |
| `scripts/parity_check.sh` | UNRELATED (SQLite↔Postgres parity) — keep |

## 3. Documentation

### 3.1 Delete outright

- `WEB_DASHBOARD.md` (carries a deprecation banner as of this scoping commit)
- `docs/WEB_API_SCHEMA_v1.md` (dashboard frontend API baseline)
- `docs/WEB_PARITY_CHECKLIST.md`, `docs/WEB_PARITY_MATRIX.md`, `docs/WEB_REBUILD_CHECKLIST.md`,
  `docs/WEB_STABLE_ROLLOUT.md` (the web↔TUI parity program + `web-stable-*` release sequence)

### 3.2 Edit (exact references)

| File | Change |
|---|---|
| `README.md` | ⚠ CLAUDE.md says README edits need explicit maintainer approval — the operator's abandonment decision covers removing dashboard content, but call it out in the PR. Remove: line 16 "Web dashboard." sentence fragment; "### Web Dashboard" section (lines ~72-90 incl. `system web` examples + WEB_DASHBOARD.md link); line ~238 + screenshot cell ~248 ("web dashboard" gallery entry); line ~334 "across CLI, web, mobile, and agent workflows" → drop "web"; line ~513 "**Actix-web** for the web dashboard" (note: was wrong anyway — it's axum) and ~514 TradingView widget bullet; docs table row ~550 for WEB_DASHBOARD.md. |
| `AGENTS.md` | remove `pftui system web` row (line ~426); line ~437 "All interfaces (TUI, Web, CLI)" → "(TUI, CLI)" |
| `ONBOARDING.md` | remove "Step 5: Set Up the Web Dashboard" (lines ~261-373 incl. exposure-matrix table + `pftui-web.service` systemd snippet), TOC entry (line 19), interfaces-table row (~33), flow mention (~48), checklist mentions (~542). Renumber subsequent steps. |
| `docs/ARCHITECTURE.md` | "Shared Intelligence Contract" (~147-152): drop the "web API reuses the same Rust structs" bullet; keep mobile bullet. If a src/web file-map entry exists elsewhere, drop it. |
| `docs/DATA-ARCHITECTURE.md` | line ~58 diagram sink "reports, briefs, newsletter, TUI/web" → "TUI"; line ~209 `mobile_timeframe_scores` note is about the MOBILE API — keep, but fix the stale wording while there (writers are `analytics/synthesis.rs` + `commands/situation.rs`, reader `analytics/situation.rs`) |
| `PRODUCT-PHILOSOPHY.md` | lines ~17, ~82, ~122 — dashboard-as-supplementary-interface claims; rewrite to TUI+CLI (+native mobile) reality |
| `PRODUCT-VISION.md` | lines ~17, ~56, ~74, ~82 — same treatment |
| `CLAUDE.md` | Documentation Index: remove the WEB_DASHBOARD.md row; "What This Is": "Three interfaces: TUI, Web Dashboard, CLI" → drop Web Dashboard |
| `docs/MOBILE-WEBAPP-DESIGN.md` | KEEP (Situation Room design realized by the server-owned analytics + native app) but prepend a note that the web/webapp surface was removed and the native mobile app + `analytics situation` inherited the design. If maintainer prefers, fold into a mobile design doc later. |
| `docs/DAEMON.md` | mobile/systemd content stays; fix line 3 "even when the TUI or web UI is closed" → "the TUI is closed" |
| `docs/AI-LAYER.md` | line ~104 "Practical Deployment" bullet "Server mode with `pftui system web` + authenticated API" — remove (or repoint at `pftui system mobile serve` if a server-mode bullet is still wanted) |
| `agents/routines/dev-agent.md` | `pftui-mobile` service checks stay (mobile kept). No routine references `system web` — verified by grep over `agents/` (all "web" hits are `web_search`). |
| `docs/KEYBINDINGS.md`, `QA-REPORT.md`, `agents/report-prompts/*` | no dashboard references — verified, no edits |
| `CHANGELOG.md` / `CHANGELOG-archive.md` / git history | historical — leave untouched; grep-zero verification excludes them |

## 4. Data layer

- **No dashboard-only tables exist. No archive-then-drop is triggered.** The web API reads shared
  tables and writes only shared tables (watchlist, transactions, alerts, journal) that have
  TUI/CLI writers and readers. The auth token is never persisted. Nothing becomes DEAD.
- `mobile_timeframe_scores` is NOT web-dashboard data: written by `src/analytics/synthesis.rs` +
  `src/commands/situation.rs`, read by `src/analytics/situation.rs` and served by the (kept)
  mobile server. Unaffected.
- `docs/db-catalog.toml` updates (hygiene — the conformance test does not validate writer paths,
  but the catalog must not point at deleted files):
  - line ~271 `[tables.journal]` writers: remove `"src/web/api.rs"`
  - line ~782 `[tables.watchlist]` (the entry containing `src/commands/demo.rs`) writers: remove `"src/web/api.rs"`
  - `[tables.mobile_timeframe_scores]` readers list is stale either way (see DATA-ARCHITECTURE row above) — fix opportunistically

## 5. Operational

- No `pftui-web` systemd unit exists in `deploy/` (only `pftui-daemon` + `pftui-mobile`); the only
  systemd/nginx snippets for the dashboard live in WEB_DASHBOARD.md + ONBOARDING.md (deleted/edited above).
- `scripts/deploy.sh` restarts `pftui-daemon pftui-mobile` only — no change.
- `/pftui-report` skill and `agents/` routines: zero dashboard references (verified).
- If any host ever enabled the WEB_DASHBOARD.md `pftui-web.service` example: removal note in the
  release CHANGELOG entry should say `systemctl disable --now pftui-web` is the operator action.

## 6. Verification (final brief)

1. `cargo build --release` — record binary size before/after (expect ≥113 KB embedded HTML + handler code; nice-to-have metric)
2. `cargo test` — full suite green; confirm `web::api` tests are gone (flake source removed)
3. `cargo clippy --all-targets` — clean (watch for now-unused deps/features)
4. `cargo test --test schema_conformance` + `pftui system schema verify` — catalog still conformant
5. `cargo test --test cli_help_smoke --test doc_commands --test analyst_routine_commands` — doc/CLI contracts hold
6. Grep-zero: `grep -rniE 'system web|web dashboard|WEB_DASHBOARD|src/web' --exclude-dir=website --exclude-dir=.git . | grep -v CHANGELOG` → empty (mobile + `data dashboard` + `web_search` hits excluded by pattern; refine as needed)
7. `pftui system mobile status` + a `system mobile serve` smoke check — mobile untouched
8. CI green without the `web-tests` job; release workflow parses (`gh workflow view` or actionlint)

**Rollback:** every brief is a focused commit — `git revert` restores fully. No data
migration occurs anywhere in this removal (no tables dropped), so rollback is loss-free.
