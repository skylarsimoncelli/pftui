<p align="center">
  <img width="1728" height="1049" alt="pftui" src="https://github.com/user-attachments/assets/a1b6b11a-5893-4b91-9ac9-e14a9c64a66b" />
</p>

<h1 align="center">pftui</h1>

<p align="center">
  <b>Your portfolio's command center.</b><br>
  Live prices. Charts. Macro data. Technical analysis. All in your terminal.<br>
  No API keys. No account. No browser tabs.<br>
</p>

<p align="center">
  <a href="https://crates.io/crates/pftui"><img src="https://img.shields.io/crates/v/pftui.svg" alt="crates.io"></a>
  <a href="https://github.com/skylarsimoncelli/pftui/releases"><img src="https://img.shields.io/github/v/release/skylarsimoncelli/pftui" alt="release"></a>
  <a href="https://github.com/skylarsimoncelli/pftui/blob/master/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="license"></a>
</p>

---

**pftui** is a terminal-based portfolio intelligence dashboard. Track stocks, crypto, commodities, forex, and funds — with real-time prices, braille charts, macro indicators, and technical analysis. One binary. Works everywhere. Looks incredible.

Think Bloomberg Terminal, but it runs in your terminal and costs nothing.

```bash
curl -fsSL https://raw.githubusercontent.com/skylarsimoncelli/pftui/master/install.sh | bash
```

Or install via [Homebrew, Cargo, Docker, apt, dnf, or Nix →](#-installation)

---

## ✨ What You Get

**📊 Live Portfolio Tracking** — Add your positions once. Prices update automatically from Yahoo Finance and CoinGecko. See your allocation, daily P&L, total gain, and 52-week range at a glance.

**📈 Beautiful Charts** — High-resolution braille charts with SMA overlays, volume bars, and gain-aware coloring. Six timeframes from 1 week to 5 years. Compare any asset against benchmarks — BTC vs S&P, Gold vs DXY, anything vs anything.

**🌍 Market & Economy Views** — 18 major market symbols and 14 macro indicators built in. Treasury yields, VIX, DXY, oil, gold, currencies — the numbers that move your portfolio, always one keypress away.

**🔍 Watchlist** — Track assets you don't own yet. Set entry targets. See when they're approaching your buy zone.

**📋 Journal** — Log your trade thesis, track predictions, search your decision history. Your future self will thank you.

**🎨 6 Stunning Themes** — Midnight, Catppuccin, Nord, Dracula, Solarized, Gruvbox. Every pixel themed. Cycle with `t`.

**🔒 Privacy First** — Percentage-only mode stores zero dollar amounts. Toggle `p` to hide values instantly. Your portfolio stays on your machine, in a local SQLite database. Nothing is sent anywhere.

**⌨️ Vim-Native** — `j`/`k` to navigate, `/` to search, `gg`/`G` to jump. If you've used vim, you already know pftui. If you haven't — you'll learn in 30 seconds.

---

## 🖥️ Views

Navigate with number keys or click the tabs.

| Key | View | |
|:---:|---|---|
| `1` | **Positions** | Your holdings — live prices, daily change, gain/loss, allocation bars, sparklines |
| `2` | **Transactions** | Buy/sell history with cost basis tracking |
| `3` | **Markets** | Broad market pulse — S&P, NASDAQ, BTC, Gold, DXY, 10Y, VIX, and more |
| `4` | **Economy** | Macro dashboard — yields, currencies, commodities, FRED data, economic calendar |
| `5` | **Watchlist** | Assets you're stalking — with price targets and proximity alerts |
| `6` | **Analytics** | Portfolio risk, scenarios, correlation matrix, stress testing |
| `7` | **Journal** | Trade log, decision history, predictions, searchable notes |

Press `Enter` on any position to open the detail popup. Press `Enter` again for the full chart.

---

## 🚀 Quick Start

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/skylarsimoncelli/pftui/master/install.sh | bash

# Launch — the setup wizard handles the rest
pftui
```

The setup wizard walks you through adding your first positions with symbol autocomplete and auto-categorization. Choose between **Full mode** (transactions with cost basis) or **Percentage mode** (allocations only, no monetary data).

**Add a position later:**
```bash
pftui add-tx
```

**Check your portfolio without opening the TUI:**
```bash
pftui value          # Quick total value + gain
pftui brief          # Formatted portfolio brief
pftui summary        # Detailed breakdown
```

---

## 📈 Charts

High-resolution Unicode braille rendering. Not ASCII art — actual data visualization in your terminal.

- **6 timeframes** — `h`/`l` to cycle: 1W, 1M, 3M, 6M, 1Y, 5Y
- **Ratio analysis** — `J`/`K` to compare: asset vs SPX, QQQ, BTC, Gold
- **Volume bars** — block characters scaled to relative volume
- **Moving averages** — SMA(20) and SMA(50) overlays
- **Gain-aware gradients** — green for gains, red for losses

---

## 🛠️ CLI

pftui works headless too. Every feature is accessible from the command line.

```bash
pftui refresh                    # Fetch latest prices
pftui value                      # Quick portfolio value
pftui brief                      # Markdown brief (great for scripts & agents)
pftui summary --period 1m        # Monthly P&L breakdown
pftui macro                      # Macro dashboard (DXY, VIX, yields, CPI)
pftui performance --vs SPY       # Portfolio vs benchmark returns
pftui calendar --impact high     # Upcoming market-moving events
pftui alerts list                # Check price & allocation alerts
pftui journal search "gold"      # Search your trade journal
pftui watch TSLA --target 300    # Watch with entry target
pftui export json                # Full portfolio export
pftui snapshot                   # Render TUI to stdout (for sharing)
```

All commands support `--json` for programmatic access.

---

## 🎨 Themes

Six built-in themes. Cycle with `t`. Your choice persists automatically.

| Theme | Vibe |
|---|---|
| **Midnight** | Deep navy with jewel-tone accents *(default)* |
| **Catppuccin** | Warm mocha with pastel highlights |
| **Nord** | Cool arctic blue-gray |
| **Dracula** | Purple-dark with vivid accents |
| **Solarized** | Teal-tinted dark with warm/cool balance |
| **Gruvbox** | Retro warm brown/orange |

---

## ⌨️ Keys

You'll pick these up in a minute. Full reference in [docs/KEYBINDINGS.md](KEYBINDINGS.md).

| Key | What it does |
|---|---|
| `j` / `k` | Navigate down / up |
| `1`-`7` | Switch view |
| `Enter` | Open detail → open chart |
| `/` | Search & filter |
| `t` | Cycle theme |
| `p` | Toggle privacy mode |
| `?` | Help |
| `q` | Quit |

Mouse works too — click tabs, click rows, scroll wheel.

---

## 🏗️ Built With

- **Rust** — fast, safe, single binary
- **ratatui** — terminal UI framework
- **SQLite** — bundled, zero-config persistence
- **Yahoo Finance & CoinGecko** — free, no API keys required

No runtime dependencies. No Node. No Python. No Docker required. Just one binary.

---

## 📦 Installation

The fastest way:

```bash
curl -fsSL https://raw.githubusercontent.com/skylarsimoncelli/pftui/master/install.sh | bash
```

<details>
<summary><b>More install options</b></summary>

### Homebrew (macOS & Linux)
```bash
brew tap skylarsimoncelli/pftui
brew install pftui
```

### Cargo
```bash
cargo install pftui
```

### Docker
```bash
docker run -it ghcr.io/skylarsimoncelli/pftui:latest
```

### Debian / Ubuntu
```bash
echo "deb [trusted=yes] https://skylarsimoncelli.github.io/pftui/apt stable main" | sudo tee /etc/apt/sources.list.d/pftui.list
sudo apt update && sudo apt install pftui
```

### Fedora / RHEL
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
cd pftui && cargo build --release
./target/release/pftui
```

</details>

---

## 🗺️ Roadmap

pftui is under active development. Here's what's coming:

- [x] Live portfolio tracking with cost basis
- [x] Braille charts with SMA overlays and ratio analysis
- [x] Markets & Economy views with 30+ indicators
- [x] 6 themes, vim keys, mouse support, privacy mode
- [x] Headless CLI (refresh, brief, summary, export, snapshot)
- [ ] Technical indicators (RSI, MACD, Bollinger Bands) on every position
- [ ] Unified alert engine (price targets, allocation drift, indicator thresholds)
- [ ] Portfolio performance tracking with benchmark comparison
- [ ] Economic calendar with impact ratings
- [ ] Correlation matrix and risk analytics
- [ ] Scenario stress testing ("what if oil hits $100?")
- [ ] Central bank & sovereign holdings tracker
- [ ] Web interface with TradingView charts
- [ ] Native OS notifications

Full roadmap: [TODO.md](../TODO.md) · Feature specs: [docs/ANALYTICS-SPEC.md](ANALYTICS-SPEC.md)

---

## 🤝 Contributing

Issues and PRs welcome. If you use pftui and have ideas, [open an issue](https://github.com/skylarsimoncelli/pftui/issues).

---

## 📝 License

MIT — do whatever you want with it.

---

<p align="center">
  <b>Stop alt-tabbing between Yahoo Finance, TradingView, and your spreadsheet.</b><br>
  <code>pftui</code> — everything in one terminal.
</p>
