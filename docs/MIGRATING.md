# Migrating Database Backends

This guide covers moving portfolio data between pftui storage backends.

## Current Status (2026-03-08)

- `sqlite` is the default and fully supported backend.
- `postgres` is fully supported via the runtime bridge layer (`database_backend`, `database_url`) and persists portfolio state in PostgreSQL.
- Existing SQLite query/storage logic is retained by materializing a local working SQLite database per run, then syncing that state to PostgreSQL on command/TUI shutdown.

## SQLite to PostgreSQL Migration Path

When PostgreSQL storage support is fully released, use this flow:

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
- PostgreSQL backend currently stores one binary SQLite state blob per active portfolio path in table `pftui_sqlite_state`.
