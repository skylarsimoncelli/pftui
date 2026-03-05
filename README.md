<h1 align="center">pftui</h1>

<p align="center">
  <b>PFTUI - The Portfolio TUI.</b><br>
  Agent-native financial battle station for equities, crypto, forex, macro, sentiment, and news.<br>
  Terminal-native. Vim-fast. Fully scriptable.<br>
</p>

<p align="center">
  <a href="https://crates.io/crates/pftui"><img src="https://img.shields.io/crates/v/pftui.svg" alt="crates.io"></a>
  <a href="https://github.com/skylarsimoncelli/pftui/releases"><img src="https://img.shields.io/github/v/release/skylarsimoncelli/pftui" alt="release"></a>
  <a href="https://github.com/skylarsimoncelli/pftui/blob/master/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="license"></a>
</p>

<p align="center">
  <img width="1724" height="1085" alt="image" src="https://github.com/user-attachments/assets/351aba69-f659-42eb-9f18-6e791731431d" />
</p>

---

**pftui** is an agent-native financial battle station built for the terminal. Track equities, crypto, commodities, forex, and funds with real-time pricing, technical overlays, macro context, sentiment signals, and event/news awareness.

Think Bloomberg Terminal ergonomics with vim-speed workflows, local-first privacy, and zero-friction automation.

```bash
curl -fsSL https://raw.githubusercontent.com/skylarsimoncelli/pftui/master/install.sh | bash
```

Or install via [Homebrew, Cargo, Docker, apt, dnf, or Nix →](#-installation)

---

## ✨ What You Get

- **📊 Portfolio Command Core** — Live positions, allocation, P&L, transaction history, watchlists, and alert-ready monitoring in one screen.

- **📈 High-density Technical Visualization** — Braille charts with multi-timeframe views, SMA overlays, ratio analysis, volume bars, and momentum context.

- **🌍 Full Market Coverage** — Equities, crypto, forex, rates, commodities, volatility, and macro indicators side by side.

- **🧠 Sentiment + Event Context** — Prediction-market style probabilities and market-moving context integrated with portfolio state.

- **🤖 Agent-native Outputs** — Generate structured JSON and markdown briefs for scripts, cron jobs, and LLM/agent pipelines.

- **⌨️ Vim-native TUI UX** — `j/k`, `gg/G`, `/` filtering, keyboard-first navigation, and optional mouse support.

- **🔒 Local-first Privacy** — SQLite on your machine, percentage-only mode, and instant value masking with `p`.

- **🎨 Polished Interface** — Multiple production-ready themes with persistent preferences.

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

## 🤖 Agent-native Workflows

Use pftui as a financial data plane for autonomous systems and scripted ops.

```bash
pftui brief --agent --json         # structured state for agents
pftui macro                        # macro regime and risk context
pftui predictions --limit 10       # sentiment/probability context
pftui export json                  # downstream automation payload
```

Everything above can be chained into shell scripts, CI jobs, local automations, or LLM tool loops.

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

You'll pick these up in a minute. Full reference in [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md).

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

pftui is under active development. Upcoming expansions include:

- Unified alert engine (price targets, allocation drift, indicator thresholds)
- Expanded performance attribution and benchmark analytics
- Scenario stress testing and correlation tooling
- Broader event/news intelligence and macro signal depth
- Optional notification and automation integrations

Full roadmap: [TODO.md](TODO.md) · Feature specs: [docs/ANALYTICS-SPEC.md](docs/ANALYTICS-SPEC.md)

---

## 🤝 Contributing

Issues and PRs welcome. If you use pftui and have ideas, [open an issue](https://github.com/skylarsimoncelli/pftui/issues).

---

## 📝 License

MIT — do whatever you want with it.

---

<p align="center">
  <b>PFTUI - The Portfolio TUI.</b><br>
  <code>pftui</code> — your terminal-native financial battle station.
</p>
