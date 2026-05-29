# Prior-release SQLite fixtures

`v0.27.0.sqlite` is a synthetic prior-release-style pftui database used by
`cargo test --test prior_release_schema`.

The fixture intentionally includes old `news_cache` and `user_predictions`
tables without the columns added during the May 28-29 autonomous-bot ramp. The
test copies this database into an isolated pftui data directory, runs
`pftui --cached-only system db-info --json` to force migrations, then smokes a
representative set of CLI commands against the migrated result.

Regeneration contract:

- Use synthetic/demo rows only. Never derive this fixture from a real local
  portfolio database.
- When a new pftui release is cut, refresh this fixture to represent the
  previous released schema so CI continues to test the last-release-to-current
  migration path.
- If a PR adds an `ALTER TABLE` migration whose behavior depends on existing
  tables or columns, update this fixture when needed and verify with
  `cargo test --test prior_release_schema`.
