# Daemon Deployment

`pftui system daemon` is the recommended always-on path for local operators who want refresh, analytics, alerts, and housekeeping to keep running even when the TUI or web UI is closed.

## What It Does

- Wakes on a small base interval (`--interval`, default `300`)
- Schedules sources independently using `daemon.cadence.*` config fields
- Runs one loop that can include price refresh, technical snapshots, key levels, analytics, alerts, and cleanup
- Writes a heartbeat file to `~/.local/share/pftui/daemon_heartbeat.json`
- Exposes health through both `pftui system daemon status --json` and `pftui data status --json`

## Quick Start

```bash
# Foreground (for testing)
pftui system daemon start

# Background with systemd (recommended)
sudo cp deploy/systemd/pftui-daemon.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now pftui-daemon
```

## Useful Commands

```bash
# Inspect current cadence defaults
pftui system config list | rg '^daemon\.cadence'

# Tighten market-price cadence while leaving slower sources alone
pftui system config set daemon.cadence.prices_interval_secs 120
pftui system config set daemon.cadence.analytics_interval_secs 120
pftui system config set daemon.cadence.alerts_interval_secs 60

# Start in foreground for debugging
pftui system daemon start --interval 30

# Machine-readable health
pftui system daemon status --json
pftui data status --json
```

## systemd Deployment

### Option A: System-level (servers, VPS, headless)

Pre-built service files are in `deploy/systemd/`. This is recommended for production servers.

```bash
# Install both services
sudo cp deploy/systemd/pftui-daemon.service /etc/systemd/system/
sudo cp deploy/systemd/pftui-mobile.service /etc/systemd/system/

# Enable on boot and start
sudo systemctl daemon-reload
sudo systemctl enable pftui-daemon pftui-mobile
sudo systemctl start pftui-daemon pftui-mobile

# Verify
sudo systemctl status pftui-daemon
sudo systemctl status pftui-mobile

# View logs
journalctl -u pftui-daemon -f
journalctl -u pftui-mobile -f
```

If you only need the daemon (no mobile API), skip the mobile service.

Edit the service files to change `User=`, `Environment=HOME`, and `ReadWritePaths=` if running as a non-root user.

### Option B: User-level (desktops, laptops)

Save as `~/.config/systemd/user/pftui-daemon.service`:

```ini
[Unit]
Description=pftui always-on analytics daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/pftui system daemon start
Restart=always
RestartSec=10
WorkingDirectory=%h
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
```

Then enable it:

```bash
systemctl --user daemon-reload
systemctl --user enable --now pftui-daemon.service
systemctl --user status pftui-daemon.service
journalctl --user -u pftui-daemon.service -f
```

If you want the service to survive logout on a desktop machine:

```bash
loginctl enable-linger "$USER"
```

## Mobile API Server

The mobile API server provides a TLS-secured REST API for the iOS app.

```bash
# Configure in config.toml
[mobile]
enabled = true
bind = "0.0.0.0"
port = 56832
cert_path = "~/.config/pftui/mobile-cert.pem"
key_path = "~/.config/pftui/mobile-key.pem"

# Generate self-signed TLS cert (if needed)
pftui system mobile cert-gen

# Create an API token
pftui system mobile token create --name "my-device" --permission write

# Start (foreground)
pftui system mobile serve

# Or use systemd (recommended)
sudo cp deploy/systemd/pftui-mobile.service /etc/systemd/system/
sudo systemctl enable --now pftui-mobile
```

## After Rebuilding the Binary

When you rebuild pftui, restart the services to pick up the new binary:

```bash
cargo build --release
cp target/release/pftui ~/.cargo/bin/pftui
sudo systemctl restart pftui-daemon pftui-mobile
```

## Cadence Guidance

Each data source has its own refresh interval, independent of the daemon wake interval:

| Source | Config Key | Default | Notes |
|---|---|---|---|
| Prices | `prices_interval_secs` | 300 | Intraday: history, technicals, levels |
| Analytics | `analytics_interval_secs` | 300 | Correlations, regime, portfolio snapshot |
| Alerts | `alerts_interval_secs` | 60 | Smart alert evaluation |
| News | `news_interval_secs` | 600 | RSS/API news feed |
| Brave News | `brave_news_interval_secs` | 14400 | Brave search news queries |
| Sentiment | `sentiment_interval_secs` | 3600 | Fear & Greed, social |
| Economy | `economy_interval_secs` | 21600 | BLS macro indicators |
| FRED | `fred_interval_secs` | 86400 | Federal Reserve data |
| FedWatch | `fedwatch_interval_secs` | 3600 | CME rate probabilities |
| COT | `cot_interval_secs` | 604800 | CFTC positioning (weekly) |
| BLS | `bls_interval_secs` | 2592000 | Bureau of Labor Statistics |
| World Bank | `worldbank_interval_secs` | 2592000 | Development indicators |
| COMEX | `comex_interval_secs` | 86400 | Vault inventory |
| On-chain | `onchain_interval_secs` | 86400 | Exchange reserves, ETF flows |
| Calendar | `calendar_interval_secs` | 86400 | Economic calendar events |
| Predictions | `predictions_interval_secs` | 3600 | Prediction market odds |
| Cleanup | `cleanup_interval_secs` | 86400 | Cache/retention housekeeping |

Use a short daemon wake interval with longer per-source cadences:

```bash
pftui system config set daemon.cadence.prices_interval_secs 120
pftui system config set daemon.cadence.news_interval_secs 900
pftui system config set daemon.cadence.alerts_interval_secs 60
pftui system config set daemon.cadence.cleanup_interval_secs 86400
```

This keeps alert evaluation and daemon health responsive without hammering slower sources.

## Troubleshooting

```bash
# Check if daemon is running
pftui system daemon status --json

# Check data freshness per source
pftui data status --json

# Check systemd logs for errors
journalctl -u pftui-daemon --since "1 hour ago" --no-pager

# Restart after config changes
sudo systemctl restart pftui-daemon
```
