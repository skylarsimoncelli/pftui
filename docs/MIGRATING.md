# Migrating Database Backends

This guide covers moving portfolio data between pftui storage backends.

## Current Status (2026-03-08)

- `sqlite` is the default and fully supported backend.
- `postgres` backend config/plumbing is available (`database_backend`, `database_url`), but query-layer storage migration is still in progress.
- If `database_backend = "postgres"` is set today, pftui will exit with a clear message until PostgreSQL storage support lands.

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
