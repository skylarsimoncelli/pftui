# Daemon Deployment

`pftui system daemon` is the recommended always-on path for local operators who want refresh, analytics, alerts, and housekeeping to keep running even when the TUI or web UI is closed.

## What It Does

- Wakes on a small base interval (`--interval`, default `300`)
- Schedules sources independently using `daemon.cadence.*` config fields
- Runs one loop that can include price refresh, technical snapshots, key levels, analytics, alerts, and cleanup
- Writes a heartbeat file to `~/.local/share/pftui/daemon_heartbeat.json`
- Exposes health through both `pftui system daemon status --json` and `pftui data status --json`

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

## Recommended systemd Unit

Save this as `~/.config/systemd/user/pftui-daemon.service`:

```ini
[Unit]
Description=pftui always-on analytics daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/pftui system daemon start --interval 30 --json
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

## Cadence Guidance

- `daemon.cadence.prices_interval_secs`: intraday cadence for prices, history backfill, technical snapshots, and levels
- `daemon.cadence.analytics_interval_secs`: correlation snapshots, regime refresh, portfolio snapshot, timeframe signals
- `daemon.cadence.alerts_interval_secs`: how often alert rules are evaluated
- `daemon.cadence.cleanup_interval_secs`: cache/retention housekeeping
- News, Brave news, predictions, sentiment, calendar, economy, COT, BLS, FRED, FedWatch, World Bank, COMEX, and on-chain each have separate cadence fields

Use a short daemon wake interval with longer per-source cadences. Example:

```bash
pftui system config set daemon.cadence.prices_interval_secs 120
pftui system config set daemon.cadence.news_interval_secs 900
pftui system config set daemon.cadence.brave_news_interval_secs 14400
pftui system config set daemon.cadence.predictions_interval_secs 3600
pftui system config set daemon.cadence.cleanup_interval_secs 86400
```

This keeps alert evaluation and daemon health responsive without hammering slower sources.
