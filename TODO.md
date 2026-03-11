# TODO — pftui

> Pick highest-priority unclaimed item. Remove when done. Update CHANGELOG.md.
> Completed items go in CHANGELOG.md only — do not mark [x] here.

---

## P0 — Analytics Engine (F31)

F31 analytics engine is complete and shipped.
Detailed implementation checklist is archived in git history/changelog.
Current references:
- `docs/ANALYTICS-ENGINE.md`
- `AGENTS.md` (Analytics Engine chapter)
- `src/commands/analytics.rs`
- `src/db/timeframe_signals.rs`

## P1 — Feature Requests

> User-requested features and high-value improvements.

### Data & Display

### CLI Enhancements


### Analytics

### Infrastructure

### Code Quality Quick Wins (audit-driven)


### F32: Native PostgreSQL Backend (epic)

Native SQLite/Postgres parity is complete and shipped. The original migration checklist is archived in git history and changelog entries.
Current authoritative validation/signoff references:
- `docs/BACKEND-PARITY.md`
- `docs/MIGRATING.md`
- `scripts/parity_check.sh`
- `.github/workflows/ci.yml` (`postgres-parity` job)

#### P32: Backend Parity Hardening (production quality)

> F32 established native Postgres paths. P32 closes remaining production-grade parity gaps:
> performance, CI validation, and docs consistency.

---

## P2 — Nice to Have

> Future improvements. Lower priority.

### TUI Polish (batch: ~4hrs total)

### Watchlist (batch: ~2hrs total)

### Scanner (batch: ~3hrs total)

### Distribution

### Other

---

## P3 — Long Term

No active long-term items right now.
