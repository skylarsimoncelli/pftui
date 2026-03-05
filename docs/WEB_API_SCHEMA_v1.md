# pftui Web API Schema v1

Version: `v1.1`  
Status: active baseline for `pftui web` frontend.

## Conventions
- Base path: `/api`
- Auth:
  - `v1` legacy mode used injected `Authorization: Bearer <token>`.
  - `v1.1` uses cookie-based session auth (`pftui_session`) + CSRF on mutating `/api/*`.
- All payloads are JSON.
- Runtime freshness (`v1.1`, implemented in Phase 2):
  - `pftui web` starts a background price/macro refresh loop using `config.refresh_interval`.
  - `pftui web` starts a background RSS ingest loop using `config.news_poll_interval`.
  - News cache cleanup (48h retention) runs during RSS ingest cycles.

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

Endpoint status markers:
- `implemented`: available in server today
- `planned`: defined contract, not implemented yet

### `GET /ui-config`
Status: `implemented`
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
Status: `implemented`
Portfolio summary and positions list.

### `GET /positions`
Status: `implemented`
Positions list only.

### `GET /watchlist`
Status: `implemented`
Watchlist entries and live cached prices.

Added fields per watchlist item:
- `distance_pct`
- `target_hit`

Response also includes:
- `meta.last_refresh_at`
- `meta.stale_after_sec`
- `meta.source_status`
- `meta.auth_required`
- `meta.transport` (`polling` default; frontend shows `Live (SSE)` when stream connected)

### `POST /watchlist`
Status: `implemented`
Add/star a symbol in watchlist.

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Request:
```json
{ "symbol": "MSFT", "category": "equity" }
```

Response:
```json
{ "ok": true, "symbol": "MSFT", "action": "added" }
```

### `DELETE /watchlist/{symbol}`
Status: `implemented`
Remove/unstar a symbol from watchlist.

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Response:
```json
{ "ok": true, "symbol": "MSFT", "action": "removed" }
```

### `GET /search?q=&limit=`
Status: `implemented`
Global asset universe search (not limited to currently loaded rows).

Response:
```json
{
  "results": [
    {
      "symbol": "MSFT",
      "name": "Microsoft",
      "category": "equity",
      "current_price": "410.12",
      "day_change_pct": "0.62",
      "is_watchlisted": false
    }
  ],
  "meta": { "transport": "polling" }
}
```

### `GET /transactions?sort_by=&sort_order=&symbol=&tx_type=&from=&to=&limit=`
Status: `implemented`
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

### `GET /stream`
Status: `implemented`
Server-Sent Events stream for live updates.

Event types:
- `quote_update`
- `panel_invalidate`
- `health`
- `heartbeat`

Example payload:
```json
{ "ts": "2026-03-05T20:10:00Z", "message": "alive" }
```

### `GET /macro`
Status: `implemented`
Macro/market indicator cards.

Response includes:
- `indicators` flat list (backward compatible)
- `sections` grouped economy payload
- `top_movers`
- `market_breadth`:
  - `up`, `down`, `flat`
  - `avg_change_pct`
  - `strongest`, `weakest` (indicator objects)
- `economy_snapshot`:
  - `bls_metrics` (latest CPI, unemployment, NFP, hourly earnings when cached)
  - `sentiment` (crypto/traditional fear-greed latest)
  - `upcoming_events` (calendar cache)
  - `predictions` (top cached prediction markets)
- `meta.*`

### `GET /alerts`
Status: `implemented`
Alert list for alerts view.

Response includes `meta.*`.

### `GET /chart/{symbol}`
Status: `implemented`
Historical chart points for symbol.

Response includes `meta.*`.

### `GET /asset/{symbol}`
Status: `implemented`
Enriched asset detail payload for drawer/popup workflows.

Response fields:
- symbol + `history_symbol` (effective symbol used for history lookup)
- `name`, `category`, `is_watchlisted`, `alert_count`
- `current_price`
- `day_change_pct`, `week_change_pct`, `month_change_pct`, `year_change_pct`
- `range_52w_low`, `range_52w_high`
- `latest_volume`, `avg_volume_30d`
- `position` summary block (if held)
- `history` chart points
- `meta.*`

### `GET /performance?timeframe=1w|1m|3m|6m|1y|5y&benchmark=spx`
Status: `implemented`
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
Status: `implemented`
Top-level summary cards and top movers.

Response includes `meta.*`.

### `GET /news?limit=&source=&category=&search=&hours=`
Status: `implemented`
News cache query for News tab.

Response includes `meta.*`.

Freshness notes:
- Backfilled automatically by web runtime RSS worker; manual `pftui refresh` is not required for active web sessions.
- Entries are deduplicated by URL and stale entries are purged during ingest cycles.

### `GET /journal?limit=&since=&tag=&symbol=&status=&search=`
Status: `implemented`
Journal query for Journal tab.

Response includes `meta.*`.

### `GET /home-tab`
Status: `implemented`
Read persisted home tab.

Response:
```json
{ "ok": true, "home_tab": "positions" }
```

### `POST /home-tab`
Status: `implemented`
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
Status: `implemented`
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

### `POST /alerts`
Status: `implemented`
Create alert rule from web UI.

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Request:
```json
{
  "rule_text": "GC=F above 5500"
}
```

Response:
```json
{
  "ok": true,
  "id": 42,
  "action": "created"
}
```

### `DELETE /alerts/{id}`
Status: `implemented`
Remove alert by id.

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Response:
```json
{ "ok": true, "id": 42, "action": "removed" }
```

### `POST /alerts/{id}/ack`
Status: `implemented`
Acknowledge triggered alert.

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Response:
```json
{ "ok": true, "id": 42, "action": "acknowledged" }
```

### `POST /alerts/{id}/rearm`
Status: `implemented`
Re-arm acknowledged/triggered alert.

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Response:
```json
{ "ok": true, "id": 42, "action": "rearmed" }
```

### `POST /watchlist`
Status: `implemented`
Add/star symbol in watchlist.

### `DELETE /watchlist/{symbol}`
Status: `implemented`
Remove/unstar symbol from watchlist.

### `PATCH /watchlist/{symbol}/target`
Status: `planned`
Set/clear watchlist target and direction.

### `POST /journal`
Status: `implemented`
Create journal entry.

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Request:
```json
{
  "content": "Thesis still valid after CPI print",
  "symbol": "MSFT",
  "tag": "thesis",
  "status": "open"
}
```

Response:
```json
{ "ok": true, "id": 18, "action": "created" }
```

### `PATCH /journal/{id}`
Status: `implemented`
Update journal entry content/status.

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Request:
```json
{ "content": "Updated note", "status": "validated" }
```

Response:
```json
{ "ok": true, "id": 18, "action": "updated" }
```

### `DELETE /journal/{id}`
Status: `implemented`
Remove journal entry.

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Response:
```json
{ "ok": true, "id": 18, "action": "removed" }
```

### `POST /transactions`
Status: `implemented`
Create transaction (full mode only).

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Request:
```json
{
  "symbol": "MSFT",
  "category": "equity",
  "tx_type": "buy",
  "quantity": "3",
  "price_per": "400",
  "currency": "USD",
  "date": "2026-03-06",
  "notes": "starter position"
}
```

Response:
```json
{ "ok": true, "id": 33, "action": "created" }
```

### `PATCH /transactions/{id}`
Status: `implemented`
Edit transaction fields (full mode only).

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Response:
```json
{ "ok": true, "id": 33, "action": "updated" }
```

### `DELETE /transactions/{id}`
Status: `implemented`
Delete transaction (full mode only).

Headers:
- `X-CSRF-Token: <csrf_token>` when auth mode is enabled.

Response:
```json
{ "ok": true, "id": 33, "action": "removed" }
```
