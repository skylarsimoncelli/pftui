# Migrating Database Backends

This guide covers moving portfolio data between pftui storage backends.

## Current Status (2026-03-09)

- `sqlite` is the default and fully supported backend.
- `postgres` runs natively through `database_backend = "postgres"` + `database_url`.
- The legacy SQLite blob bridge (`pftui_sqlite_state`) has been removed.
- Runtime interfaces (CLI/TUI/Web) execute against the active backend; SQLite is not used as a hidden bridge when Postgres is selected.
- `pftui db-info` reports active backend, connection target, and per-table row counts.

## SQLite to PostgreSQL Migration Path

To move an existing SQLite portfolio into PostgreSQL:

1. Export from existing SQLite profile:

```bash
pftui export json --output /tmp/pftui-export.json
```

2. Run setup and choose PostgreSQL backend:

```bash
pftui setup
```

3. Import exported snapshot into the new backend:

```bash
pftui import /tmp/pftui-export.json --mode replace
```

4. Validate:

```bash
pftui refresh
pftui value
pftui summary
```

## PostgreSQL Back to SQLite

Use the same process in reverse:

1. `pftui export json`
2. `pftui setup` (choose SQLite)
3. `pftui import ... --mode replace`

## Notes

- Backups are strongly recommended before backend switches.
- `replace` overwrites existing portfolio data in the destination backend.
- `merge` can be used for additive imports when appropriate.
- Backend switching between SQLite and PostgreSQL is done via `export`/`import`.
