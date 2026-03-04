# pftui Web Dashboard

## Quick Start

```bash
# Start the web server (with authentication)
pftui web

# Custom port and bind address
pftui web --port 3000 --bind 0.0.0.0

# Disable authentication (localhost only!)
pftui web --no-auth
```

## Features

### REST API

9 endpoints serving portfolio and market data:

- `GET /api/portfolio` — Full portfolio summary with positions, total value, gains
- `GET /api/positions` — Detailed position list
- `GET /api/watchlist` — Watchlist items with current prices
- `GET /api/transactions` — Transaction history (full mode only)
- `GET /api/macro` — 8 macro indicators (SPX, Nasdaq, VIX, Gold, Silver, BTC, DXY, 10Y)
- `GET /api/alerts` — Active alerts
- `GET /api/chart/:symbol` — Price history for charting
- `GET /api/performance` — Portfolio performance metrics (placeholder)
- `GET /api/summary` — Brief portfolio summary with top movers

### Frontend

- **Dark theme** matching the TUI aesthetic
- **Responsive layout** (2-column desktop, 1-column mobile)
- **TradingView charts** — Interactive professional-grade charts using the free Advanced Chart Widget
- **Searchable positions table** — Filter by symbol or name
- **Click-to-chart** — Click any position, watchlist item, or macro indicator to view chart
- **Auto-refresh** — Data updates every 60 seconds
- **Macro panel** — Key market indicators at a glance
- **Watchlist** — Quick access to tracked symbols

## Authentication

By default, the server generates a bearer token and prints it on startup:

```
🔐 Authentication enabled. Use token: pftui_65e3f2a8
   Add header: Authorization: Bearer pftui_65e3f2a8
```

For API requests:
```bash
curl -H "Authorization: Bearer pftui_65e3f2a8" http://localhost:8080/api/portfolio
```

The web UI at `/` doesn't require auth (static HTML). API endpoints require the token.

To disable auth entirely (localhost only):
```bash
pftui web --no-auth
```

## Architecture

### Backend (Rust + axum)

- **src/web/mod.rs** — Module exports
- **src/web/server.rs** — axum server setup, routes, CORS
- **src/web/api.rs** — REST API handlers (9 endpoints)
- **src/web/auth.rs** — Bearer token middleware
- **src/web/static/index.html** — Embedded frontend (via `include_str!()`)

All data access goes through existing `db/*` and `models/*` modules — no duplication.

### Frontend (Vanilla HTML/CSS/JS)

- Single-page application
- No build tooling required
- TradingView widget loaded from CDN
- 600+ lines of clean, readable code
- Mobile-friendly responsive grid

## Development Notes

### Adding API Endpoints

1. Add handler function in `src/web/api.rs`
2. Define response struct (with `#[derive(Serialize)]`)
3. Register route in `src/web/server.rs`
4. Access via existing db/models functions

Example:
```rust
pub async fn get_foo(
    State(state): State<Arc<AppState>>,
) -> Result<Json<FooResponse>, (StatusCode, String)> {
    let conn = state.get_conn().map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e))
    })?;
    
    let data = db::foo::get_all(&conn).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to load foo: {}", e))
    })?;
    
    Ok(Json(FooResponse { data }))
}
```

### TradingView Integration

The frontend uses the free TradingView Advanced Chart Widget:

```javascript
chartWidget = new TradingView.widget({
    symbol: 'AAPL',
    interval: 'D',
    theme: 'dark',
    container_id: 'tradingview-chart',
    // ... config
});
```

No API key needed. Falls back to internal chart data via `/api/chart/:symbol` if TradingView is unavailable.

## Production Deployment

### Systemd Service

Create `/etc/systemd/system/pftui-web.service`:

```ini
[Unit]
Description=pftui web dashboard
After=network.target

[Service]
Type=simple
User=pftui
WorkingDirectory=/home/pftui
ExecStart=/usr/local/bin/pftui web --port 8080 --bind 127.0.0.1
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl enable pftui-web
sudo systemctl start pftui-web
```

### Reverse Proxy (nginx)

```nginx
server {
    listen 80;
    server_name portfolio.example.com;
    
    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

### Security Considerations

- **Never use `--no-auth` on public networks**
- Use HTTPS (Let's Encrypt via nginx/caddy reverse proxy)
- Bind to `127.0.0.1` if only accessing locally
- Rotate auth token periodically (restart server)
- Consider adding rate limiting for public deployments

## Future Enhancements

- [ ] PID file management
- [ ] Graceful shutdown signal handling
- [ ] API endpoint tests
- [ ] WebSocket support for live updates
- [ ] User-configurable auth tokens (vs auto-generated)
- [ ] Multi-user support with separate portfolios
- [ ] Export dashboard as PDF/PNG
- [ ] Custom chart intervals and indicators

## Troubleshooting

### Port already in use
```bash
# Check what's using the port
lsof -i :8080

# Use a different port
pftui web --port 3000
```

### Can't access from other devices
```bash
# Bind to all interfaces (use with caution!)
pftui web --bind 0.0.0.0
```

### TradingView charts not loading
- Check browser console for errors
- Verify TradingView CDN is accessible
- Fallback: use `/api/chart/:symbol` endpoint data
