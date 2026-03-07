<h1 align="center">pftui</h1>

<p align="center">
  <b>The Portfolio Intelligence Platform.</b><br>
  Built for human operators and their AI agents to collaborate on portfolio management.<br>
  TUI · Web Dashboard · CLI · Agent API · Local-first.<br>
</p>

<p align="center">
  <a href="https://crates.io/crates/pftui"><img src="https://img.shields.io/crates/v/pftui.svg" alt="crates.io"></a>
  <a href="https://github.com/skylarsimoncelli/pftui/releases"><img src="https://img.shields.io/github/v/release/skylarsimoncelli/pftui" alt="release"></a>
  <a href="https://github.com/skylarsimoncelli/pftui/blob/master/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="license"></a>
</p>

<p align="center">
  <img width="1724" height="1085" alt="pftui terminal UI" src="https://github.com/user-attachments/assets/351aba69-f659-42eb-9f18-6e791731431d" />
</p>

---

**pftui** is a portfolio intelligence platform designed for two operators working together: **a human** who makes the decisions, and **an AI agent** that does the research, monitors the markets, and keeps the system running.

For the human: a full-featured terminal UI and a sleek responsive web dashboard — both designed for maximum information density with zero friction.

For the agent: every feature exposed via CLI with structured JSON output, custom data models, and a local SQLite database that serves as the single source of truth for portfolio state.

```bash
curl -fsSL https://raw.githubusercontent.com/skylarsimoncelli/pftui/master/install.sh | bash
```

Or install via [Homebrew, Cargo, Docker, apt, dnf, or Nix →](#-installation)

---

## 🧑‍💻 For Human Operators

### Terminal UI — Your Financial Battle Station

A Bloomberg Terminal-grade interface in your terminal. Vim-native keybindings, 11 hand-tuned themes, braille charts, and every data point you need at a glance.

| Key | View | What You Get |
|:---:|---|---|
| `1` | **Positions** | Live prices, daily P&L, allocation bars, sparklines, RSI, 52W range |
| `2` | **Transactions** | Buy/sell history with cost basis tracking |
| `3` | **Markets** | S&P, NASDAQ, BTC, Gold, DXY, VIX, oil, copper — the macro pulse |
| `4` | **Economy** | Yields, currencies, commodities, FRED data, economic calendar, prediction markets, F&G |
| `5` | **Watchlist** | Assets you're stalking with price targets and proximity alerts |
| `6` | **News** | Aggregated financial news from RSS feeds — Reuters, CoinDesk, Bloomberg |
| `7` | **Journal** | Trade log, decision history, predictions, searchable notes |

**Charts:** High-resolution Unicode braille rendering with SMA overlays, Bollinger bands, volume bars, ratio analysis (vs SPX, QQQ, BTC, Gold), and 6 timeframes (1W → 5Y). Not ASCII art — actual data visualization.

**Themes:** 11 production-ready themes — Midnight, Catppuccin, Nord, Dracula, Solarized, Gruvbox, Inferno, Neon, Hacker, Pastel, Miasma. Cycle with `t`. Your choice persists.

**Privacy:** Press `p` to instantly mask all monetary values. Percentage-only mode stores no dollar amounts at all.

**Keys:** Vim-native — `j`/`k`, `gg`/`G`, `/` search, `Enter` to drill down, `Esc` to back out, `?` for help. Mouse works too. Full reference: [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md)

### Web Dashboard — Your Portfolio on Any Screen

A responsive web interface serving the same data as the TUI, accessible from any browser.

```bash
pftui web                          # Start on localhost:8080 with auth
pftui web --port 3000 --bind 0.0.0.0   # Custom port, remote access
```

- **Responsive layout** — 2-column desktop, 1-column mobile
- **TradingView charts** — professional interactive charts via the free Advanced Chart Widget
- **Click-to-chart** — click any position, watchlist item, or macro indicator
- **Auto-refresh** — data updates every 60 seconds
- **Dark theme** matching the TUI aesthetic
- **REST API** — 9 JSON endpoints for custom integrations

Details: [WEB_DASHBOARD.md](WEB_DASHBOARD.md)

---

## 🤖 For AI Agents

pftui is designed to be the data backbone for AI-powered financial workflows. Every feature in the TUI and web dashboard has a CLI counterpart with structured JSON output. The SQLite database is the single source of truth — agents read from it, write to it, and build on top of it.

### Portfolio & Market Data

```bash
pftui refresh                      # Fetch all prices + macro + sentiment + news
pftui brief --json                 # Full portfolio state as structured JSON
pftui value --json                 # Total value with category breakdown
pftui summary --json               # Detailed position-level breakdown
pftui macro --json                 # DXY, VIX, yields, commodities, derived ratios
pftui watchlist --json             # All watched symbols with prices
pftui movers --json                # Today's significant moves (held + watchlist)
pftui predictions --json           # Polymarket prediction market odds
pftui sentiment --json             # Fear & Greed indices + COT positioning
pftui news --json                  # Aggregated financial news
pftui supply --json                # COMEX inventory data
pftui global --json                # World Bank macro (BRICS, G7)
pftui performance --json           # Returns (1D, MTD, QTD, YTD)
pftui drift --json                 # Allocation drift vs targets
pftui history --date 2026-03-01 --json  # Historical snapshot
pftui status --json                # Data source freshness
```

### Portfolio Management

```bash
pftui add-tx --symbol AAPL --category equity --tx-type buy \
  --quantity 100 --price 175.50 --date 2026-03-01 --notes "Earnings dip"
pftui remove-tx 42
pftui set-cash USD 50000
pftui watch TSLA --target 300
pftui unwatch TSLA
pftui target set AAPL --target 15  # Target allocation %
pftui rebalance --json             # Suggested trades to hit targets
pftui alerts add "BTC above 100000"
pftui journal add --content "Gold thesis validated by CPI" --tag macro
```

### Data Architecture

```
~/.local/share/pftui/pftui.db     # SQLite — single source of truth
├── transactions                   # Buy/sell records with cost basis
├── price_cache                    # Latest spot prices
├── price_history                  # Daily OHLCV history
├── watchlist                      # Tracked symbols
├── alerts                         # Price/allocation alerts
├── targets                        # Target allocation percentages
├── journal_entries                # Trade journal + notes
├── calendar_events                # Economic calendar
├── news_cache                     # RSS feed articles
├── sentiment_cache                # F&G indices
├── prediction_cache               # Polymarket odds
├── cot_cache                      # CFTC COT positioning
├── comex_cache                    # COMEX inventory
├── bls_cache                      # BLS economic data
├── worldbank_cache                # Global macro indicators
└── onchain_cache                  # BTC on-chain + ETF flows
```

PostgreSQL support coming soon for multi-agent and production deployments.

### Data Sources — Zero Configuration

Every data source works out of the box with no API keys:

| Source | Data | Update Cadence |
|---|---|---|
| Yahoo Finance | Equities, ETFs, forex, crypto, commodities | Real-time |
| CoinGecko | Crypto prices, market cap, volume | Real-time |
| Polymarket | Prediction market probabilities | 15-min |
| CFTC Socrata | Commitments of Traders (COT) positioning | Weekly |
| Alternative.me | Crypto Fear & Greed Index | Daily |
| BLS API v1 | CPI, unemployment, NFP, wages | Monthly |
| World Bank | GDP, debt/GDP, reserves for 8 economies | Quarterly |
| CME Group | COMEX gold/silver inventory | Daily |
| Blockchair | BTC on-chain data | Real-time |
| RSS Feeds | Reuters, CoinDesk, Bloomberg, Kitco, CNBC | 10-min |

Optional API keys unlock additional sources (Finnhub, FRED, Alpha Vantage). See [docs/API-SOURCES.md](docs/API-SOURCES.md).

---

## 🚀 Quick Start

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/skylarsimoncelli/pftui/master/install.sh | bash

# Launch — the setup wizard handles the rest
pftui
```

The setup wizard walks you through adding your first positions with symbol autocomplete and auto-categorization. Choose between **Full mode** (transactions with cost basis) or **Percentage mode** (allocations only, no monetary data).

---

## 🏗️ Built With

- **Rust** — fast, safe, single binary
- **ratatui** — terminal UI framework
- **SQLite** — bundled, zero-config persistence
- **Actix-web** — web dashboard server
- **TradingView Widget** — interactive web charts

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

- Configurable homepage (portfolio-first vs watchlist-first)
- Full chart search for any symbol (`/` → chart → quick-add)
- Portfolio scenario engine (what-if modeling)
- PostgreSQL backend for multi-agent deployments
- Enhanced correlation and risk analytics
- Broader economic calendar and event intelligence

Full roadmap: [TODO.md](TODO.md) · Feature specs: [docs/ANALYTICS-SPEC.md](docs/ANALYTICS-SPEC.md)

---

## 📖 Documentation

| Document | Description |
|---|---|
| [AGENTS.md](AGENTS.md) | Agent operator guide — setup, workflows, integration patterns |
| [CLAUDE.md](CLAUDE.md) | Development guide for AI coding agents |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Code architecture, file map, line ranges |
| [docs/ANALYTICS-SPEC.md](docs/ANALYTICS-SPEC.md) | Feature specifications |
| [docs/API-SOURCES.md](docs/API-SOURCES.md) | Free data source reference |
| [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md) | Full keyboard shortcut reference |
| [docs/VISION.md](docs/VISION.md) | Design principles and philosophy |
| [WEB_DASHBOARD.md](WEB_DASHBOARD.md) | Web dashboard setup and API reference |
| [TODO.md](TODO.md) | Development backlog |
| [CHANGELOG.md](CHANGELOG.md) | Release history |

---

## 🤝 Contributing

Issues and PRs welcome. If you use pftui and have ideas, [open an issue](https://github.com/skylarsimoncelli/pftui/issues).

---

## 📝 License

MIT — do whatever you want with it.

---

<p align="center">
  <b>pftui</b> — portfolio intelligence for humans and agents.<br>
</p>
