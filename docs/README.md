# pftui

**Your portfolio, in the terminal. Live prices. Braille charts. Zero fluff.**

<img width="1606" height="1025" alt="image" src="https://github.com/user-attachments/assets/375d1af7-8050-4879-aee0-0e04f1dce125" />

A maximalist terminal dashboard for tracking your investments — equities, crypto, forex, commodities, funds — with live market data, ratio analysis, 6 hand-tuned color themes, and vim-native keybindings. Built in Rust.

Think Bloomberg Terminal meets btop.

## Why pftui?

- **Live everything** — spot prices from Yahoo Finance and CoinGecko, auto-refreshing on a configurable interval. Cached prices on startup so you never stare at a loading screen.
- **Braille charts** — high-resolution Unicode braille rendering with gain-aware gradient coloring, volume bars, SMA(20)/SMA(50) overlays, and 6 selectable timeframes (1W → 5Y).
- **Ratio analysis** — compare any asset against benchmarks. BTC/SPX, AAPL/QQQ, Gold/BTC — cycle through variants with `J`/`K`.
- **5 views** — Positions, Transactions, Markets (18 symbols), Economy (14 macro indicators), and a Watchlist for assets you're eyeing.
- **Privacy mode** — percentage-only mode stores zero monetary data. Or toggle `p` to hide values in-session. Your portfolio, your business.
- **6 themes** — Midnight, Catppuccin Mocha, Nord, Dracula, Solarized Dark, Gruvbox. Every pixel is themed. Cycle with `t`.
- **Vim-native** — `j`/`k`, `gg`/`G`, `Ctrl+d`/`Ctrl+u`, `/` search, `Esc` to close. If you know vim, you already know pftui.
- **Financially precise** — `rust_decimal` everywhere. No floating point. No rounding errors. Decimal strings in SQLite.
- **Single binary** — Rust + bundled SQLite. No runtime dependencies. No API keys required.

## Installation

### Homebrew (macOS & Linux)
```bash
brew tap skylarsimoncelli/pftui
brew install pftui
```

### Cargo (Rust)
```bash
cargo install pftui
```

### Docker
```bash
docker run -it ghcr.io/skylarsimoncelli/pftui:latest
```

### Debian/Ubuntu (apt)
```bash
echo "deb [trusted=yes] https://skylarsimoncelli.github.io/pftui/apt stable main" | sudo tee /etc/apt/sources.list.d/pftui.list
sudo apt update && sudo apt install pftui
```

### Fedora/RHEL (yum/dnf)
```bash
sudo tee /etc/yum.repos.d/pftui.repo << 'EOF'
[pftui]
name=pftui
baseurl=https://skylarsimoncelli.github.io/pftui/rpm
enabled=1
gpgcheck=0
EOF
sudo dnf install pftui
```

### Nix
```bash
nix run github:skylarsimoncelli/pftui
```

### From Source
```bash
git clone https://github.com/skylarsimoncelli/pftui.git
cd pftui
cargo build --release
./target/release/pftui
```

## Quick Start

```bash
# Launch (runs setup wizard on first run)
pftui
```

The setup wizard walks you through adding your positions — with symbol autocomplete and category auto-detection. Choose **Full mode** (buy/sell transactions with cost basis tracking) or **Percentage mode** (allocation percentages only, no monetary data stored).

## Usage

```bash
pftui                          # Launch TUI
pftui setup                    # Re-run setup wizard
pftui add-tx                   # Add a transaction
pftui remove-tx <id>           # Remove a transaction
pftui list-tx                  # List all transactions
pftui summary                  # Portfolio summary (stdout)
pftui summary --period 1w      # Weekly P&L
pftui summary --group-by category --period 1m  # Monthly P&L by category
pftui export csv               # Export as CSV
pftui export json              # Export as JSON
pftui refresh                   # Fetch live prices (headless)
pftui value                     # Quick portfolio value + gain
pftui brief                     # Markdown portfolio brief (for agents/scripts)
pftui watch AAPL               # Add to watchlist
pftui unwatch AAPL             # Remove from watchlist
```

## Views

| Key | View | What it shows |
|---|---|---|
| `1` | **Positions** | Holdings with live prices, daily change %, gain %, allocation %, 52-week range, sparkline trends |
| `2` | **Transactions** | Buy/sell history with date, quantity, price |
| `3` | **Markets** | 18 major symbols — SPX, NDX, BTC, Gold, DXY, 10Y yield, and more |
| `4` | **Economy** | 14 macro indicators — treasury yields, currencies, commodities, VIX |
| `5` | **Watchlist** | Track assets without holding them |

Press `Enter` on any position for a detail popup (price, cost basis, gain, recent transactions). Press `Enter` again to open the chart.

## Charts

High-resolution braille charts with:
- **6 timeframes**: `h`/`l` to cycle — 1W, 1M, 3M, 6M, 1Y, 5Y
- **Ratio variants**: `J`/`K` to cycle — compare against SPX, QQQ, BTC, Gold
- **Volume bars**: block characters scaled to relative volume
- **Moving averages**: SMA(20) and SMA(50) overlaid on single-symbol charts
- **Gain-aware coloring**: green gradients for gains, red for losses

## Themes

Six built-in themes, cycled with `t`:

| Theme | Vibe |
|---|---|
| **Midnight** | Deep navy with jewel-tone accents (default) |
| **Catppuccin** | Warm Mocha with pastel highlights |
| **Nord** | Cool arctic blue-gray |
| **Dracula** | Purple-dark with vivid accents |
| **Solarized** | Teal-tinted dark with warm/cool balance |
| **Gruvbox** | Retro warm brown/orange |

Each theme defines 28 color slots covering every UI element. Your choice persists to config automatically.

## Keybindings

Essential keys to get started:

| Key | Action |
|---|---|
| `j`/`k` | Navigate up/down |
| `gg` / `G` | Jump to top / bottom |
| `Ctrl+d` / `Ctrl+u` | Half-page scroll |
| `/` | Search / filter |
| `Enter` | Position detail → chart |
| `t` | Cycle theme |
| `p` | Toggle privacy |
| `?` | Help overlay |
| `q` | Quit |

Full reference: **[docs/KEYBINDINGS.md](KEYBINDINGS.md)**

## Architecture

Rust + ratatui + crossterm. Price service runs on a dedicated thread with Tokio, communicating via channels. TUI renders at ~60fps. SQLite (bundled) for persistence. Zero external runtime dependencies.

Full technical docs: **[docs/ARCHITECTURE.md](ARCHITECTURE.md)**

## Inspirations

- [btop](https://github.com/aristocratos/btop) — aesthetic density and theme systems
- [lazygit](https://github.com/jesseduffield/lazygit) — keyboard-driven TUI UX
- [tickrs](https://github.com/tarkah/tickrs) — terminal stock charts with braille
- [ticker](https://github.com/achannarasappa/ticker) — simple terminal stock tracker
- Bloomberg Terminal — information density, ratio analysis, multi-panel layout

## License

MIT
