# systemd Service Files

Production-ready systemd units for running pftui as a background service.

## Services

| Service | Description | Command |
|---|---|---|
| `pftui-daemon` | Data refresh cycle (prices, news, economy, technicals, alerts) | `pftui system daemon start` |
| `pftui-mobile` | iOS mobile API server (TLS, port configurable in config.toml) | `pftui system mobile serve` |

## Installation

```bash
# Copy service files
sudo cp pftui-daemon.service /etc/systemd/system/
sudo cp pftui-mobile.service /etc/systemd/system/

# Reload systemd, enable on boot, and start
sudo systemctl daemon-reload
sudo systemctl enable pftui-daemon pftui-mobile
sudo systemctl start pftui-daemon pftui-mobile
```

## Management

```bash
# Check status
sudo systemctl status pftui-daemon
sudo systemctl status pftui-mobile

# View logs (live tail)
journalctl -u pftui-daemon -f
journalctl -u pftui-mobile -f

# Restart after binary rebuild
sudo systemctl restart pftui-daemon pftui-mobile

# Stop
sudo systemctl stop pftui-daemon pftui-mobile
```

## After Rebuilding the Binary

When you rebuild pftui (`cargo build --release`), restart the services to pick up the new binary:

```bash
cargo build --release
cp target/release/pftui ~/.cargo/bin/pftui
sudo systemctl restart pftui-daemon pftui-mobile
```

## Configuration

Both services read from `~/.config/pftui/config.toml`. Key settings:

- **Daemon refresh interval:** `refresh_interval_secs` (default: 300s)
- **Mobile API port:** `[mobile] port` (default: 56832)
- **Mobile TLS:** `[mobile] cert_path` and `key_path`

## Customisation

The service files assume:
- Binary at `/usr/local/bin/pftui` (symlink to `~/.cargo/bin/pftui`)
- Config at `/root/.config/pftui/config.toml`
- Running as root (adjust `User=` for non-root setups)

For non-root installations, update the `User=`, `Environment=HOME`, `ReadWritePaths=`, and `ProtectHome=` directives.

## Why systemd over screen?

- **Auto-restart on crash:** `Restart=on-failure` with 10s delay
- **Survives reboots:** `enable` adds to boot sequence
- **Structured logging:** `journalctl` with timestamps, filtering, rotation
- **Process isolation:** systemd cgroup management
- **No orphaned sessions:** Sub-agents can't accidentally kill the daemon
