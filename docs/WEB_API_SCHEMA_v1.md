# pftui Web API Schema v1

Version: `v1.1`  
Status: active baseline for `pftui web` frontend.

## Conventions
- Base path: `/api`
- Auth:
  - `v1` legacy mode used injected `Authorization: Bearer <token>`.
  - `v1.1` uses cookie-based session auth (`pftui_session`) + CSRF on mutating `/api/*`.
- All payloads are JSON.

## Auth Endpoints (`/auth`)

### `POST /auth/login`
Create a browser session when auth is enabled.

Request:
```json
{ "token": "pftui_login_token" }
```

Response:
```json
{
  "ok": true,
  "issued_at": "2026-03-05T18:20:00Z",
  "expires_at": "2026-03-06T02:20:00Z",
  "csrf_token": "csrf_...",
  "auth_mode": "session"
}
```

### `POST /auth/logout`
Invalidates the active session cookie.

Response:
```json
{ "ok": true }
```

### `GET /auth/session`
Returns the current session state.

Response:
```json
{
  "authenticated": true,
  "issued_at": "2026-03-05T18:20:00Z",
  "expires_at": "2026-03-06T02:20:00Z",
  "csrf_token": "csrf_...",
  "auth_mode": "session"
}
```

### `GET /auth/csrf`
Returns CSRF token for the current session.

Response:
```json
{ "csrf_token": "csrf_..." }
```

### Standardized Auth Error
When auth fails:
```json
{
  "code": "session_missing",
  "message": "Authentication required",
  "relogin_required": true
}
```

## Endpoints

### `GET /ui-config`
Bootstrap UI contract for tabs, themes, and startup context.

Response:
```json
{
  "tabs": ["Positions", "Transactions", "Markets", "Economy", "Watchlist", "Alerts", "News", "Journal"],
  "themes": [{ "name": "midnight", "colors": { "bg_primary": "#000000" } }],
  "current_theme": "midnight",
  "home_tab": "positions"
}
```

### `GET /portfolio`
Portfolio summary and positions list.

### `GET /positions`
Positions list only.

### `GET /watchlist`
Watchlist entries and live cached prices.

Added fields per watchlist item:
- `distance_pct`
- `target_hit`

Response also includes:
- `meta.last_refresh_at`
- `meta.stale_after_sec`
- `meta.source_status`
- `meta.auth_required`
- `meta.transport` (`polling` in `v1.1`; `sse` planned)

### `GET /transactions?sort_by=&sort_order=&symbol=&tx_type=&from=&to=&limit=`
Transaction history (full mode only).

Query:
- `sort_by`: `date|symbol|type|qty|price|fee`
- `sort_order`: `asc|desc`
- `symbol`: symbol filter
- `tx_type`: `buy|sell`
- `from` / `to`: date range (`YYYY-MM-DD`)
- `limit`: max rows

Response includes:
- `sort_by`
- `sort_order`
- `meta.*`

### `GET /macro`
Macro/market indicator cards.

Response includes:
- `indicators` flat list (backward compatible)
- `sections` grouped economy payload
- `top_movers`
- `meta.*`

### `GET /alerts`
Alert list for alerts view.

Response includes `meta.*`.

### `GET /chart/{symbol}`
Historical chart points for symbol.

Response includes `meta.*`.

### `GET /performance?timeframe=1w|1m|3m|6m|1y|5y&benchmark=spx`
Portfolio curve and metrics.

Response fields:
- `daily_values`: `{date, value}[]`
- `metrics.total_return_pct`
- `metrics.max_drawdown_pct`
- `estimated`: `true` when fallback reconstruction is used
- `coverage_pct`: `0..100` estimated data coverage
- `source`: `"snapshots"` or `"estimated_history"`
- `benchmark_values`: optional normalized benchmark curve when `benchmark=spx`
- `meta.*`

### `GET /summary`
Top-level summary cards and top movers.

Response includes `meta.*`.

### `GET /news?limit=&source=&category=&search=&hours=`
News cache query for News tab.

Response includes `meta.*`.

### `GET /journal?limit=&since=&tag=&symbol=&status=&search=`
Journal query for Journal tab.

Response includes `meta.*`.

### `GET /home-tab`
Read persisted home tab.

Response:
```json
{ "ok": true, "home_tab": "positions" }
```

### `POST /home-tab`
Persist preferred home tab across TUI/web.

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Request:
```json
{ "home_tab": "positions" }
```

Response:
```json
{ "ok": true, "home_tab": "positions" }
```

### `POST /theme`
Persist preferred theme across TUI/web.

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Request:
```json
{ "theme": "midnight" }
```

Response:
```json
{ "ok": true, "theme": "midnight" }
```
