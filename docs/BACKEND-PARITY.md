# Backend Parity Signoff

This document defines the supported parity scope between SQLite and PostgreSQL, plus a practical verification runbook.

## Supported Scope

- Backend selection is controlled by config:
  - `database_backend = "sqlite"` (default)
  - `database_backend = "postgres"` with `database_url`
- CLI, TUI, and web API execute against the active backend.
- Export/import switching between backends is supported (`pftui export json` -> `pftui import ... --mode replace`).

## Verification Runbook

### 1. Backend config sanity

```bash
pftui config get database_backend
pftui db-info
```

### 2. Data path sanity

```bash
pftui import /tmp/pftui-export.json --mode replace
pftui value --json
pftui summary --json
pftui watchlist --json
pftui drift --json
```

### 3. Cross-backend parity check

Use the acceptance script:

```bash
PFTUI_TEST_POSTGRES_URL=postgres://... scripts/parity_check.sh
```

The script runs key commands on isolated sqlite and postgres profiles and diffs normalized JSON output.

CI also runs this script in the Postgres parity job, using `target/debug/pftui` as `PFTUI_BIN`.

## Backend Switch Runbook

SQLite -> PostgreSQL (same steps in reverse for PostgreSQL -> SQLite):

```bash
pftui export json --output /tmp/pftui-export.json
pftui setup          # choose postgres and set database_url
pftui import /tmp/pftui-export.json --mode replace
pftui refresh
pftui value
pftui summary
```

## Known Intentional Differences

- IDs and timestamp metadata (`id`, `created_at`, `updated_at`, `fetched_at`) are backend-generated and may differ.
- Ordering may differ where SQL does not define a strict `ORDER BY` in a query path.

For parity validation, compare semantic fields and use normalized output where metadata is stripped.
