<p align="center">
  <b>pftui</b><br>
  Portfolio intelligence for humans and their AI agents.<br>
  <a href="https://pftui.com">pftui.com</a>
</p>

<p align="center">
  <a href="https://crates.io/crates/pftui"><img src="https://img.shields.io/crates/v/pftui.svg" alt="crates.io"></a>
  <a href="https://github.com/skylarsimoncelli/pftui/releases"><img src="https://img.shields.io/github/v/release/skylarsimoncelli/pftui" alt="release"></a>
  <a href="https://github.com/skylarsimoncelli/pftui/blob/master/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="license"></a>
</p>

---

**pftui** is a local-first portfolio terminal for investors and their AI agents.
Track positions, markets, macro, sentiment, news, and analytics in one system.
Built for self-directed investors and technical operators, it gives you a fast terminal UI, a browser dashboard, and a structured CLI your AI agent can work with directly.

---

## Why pftui

- **One place for everything** — portfolio tracking, watchlists, markets, macro, news, and research state
- **Local-first by default** — your data stays with you, not inside a hosted black box
- **Fast and practical** — built for daily use in the terminal, not just occasional reporting
- **Agent-ready** — every major feature has CLI access and JSON output
- **Compounds over time** — the longer you run it, the more useful your local dataset becomes

---

### 🚀 Recommended Setup

The easiest way to get started is to let your AI agent set up pftui with you.

Give it this prompt:

> Read AGENTS.md in the pftui repo: https://github.com/skylarsimoncelli/pftui
>
> Install pftui, help me set up pftui with my portfolio and watchlist, and walk me through the functionality

If you use **Claude Code**, **Codex**, **OpenClaw**, or another coding agent, it can read [AGENTS.md](AGENTS.md), install pftui, and guide you through the initial setup.

Manual install options are below in [Installation](#-installation).

---

## 🧑‍💻 For Human Operators

### Terminal UI

pftui’s core experience is a fast, keyboard-driven terminal interface built for daily portfolio and market monitoring.

| Key | View | What You Get |
|:---:|---|---|
| `1` | **Positions** | Live prices, daily P&L, allocation bars, sparklines, RSI, 52W range |
| `2` | **Transactions** | Buy/sell history with cost basis tracking |
| `3` | **Markets** | S&P, NASDAQ, BTC, Gold, DXY, VIX, oil, copper |
| `4` | **Economy** | Yields, currencies, commodities, FRED data, economic calendar, prediction markets, F&G |
| `5` | **Watchlist** | Symbols with price targets and proximity alerts |
| `6` | **News** | Aggregated financial news from major feeds |
| `7` | **Journal** | Trade log, decision history, predictions, searchable notes |

**Highlights**

- **Braille charts** with SMA overlays, Bollinger bands, volume bars, ratio analysis, and 6 timeframes
- **11 built-in themes** including Midnight, Catppuccin, Nord, Dracula, Solarized, Gruvbox, Inferno, Neon, Hacker, Pastel, and Miasma
- **Privacy mode** with instant value masking and percentage-only operation
- **Vim-style navigation** with mouse support

Full reference: [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md)

### Web Dashboard

pftui also includes a responsive browser interface backed by the same local data.

```bash
pftui web                          # Start on localhost:8080 with auth
pftui web --port 3000 --bind 0.0.0.0   # Custom port, remote access
```

Features include:

- responsive desktop and mobile layout
- TradingView charts
- click-through asset views
- auto-refreshing data
- all native TUI themes
- REST API endpoints for integrations

Details: [WEB_DASHBOARD.md](WEB_DASHBOARD.md)

---

## 🤖 For AI Agents

pftui gives AI agents structured access to the same system the human operator uses.

Your agent can refresh market data, inspect portfolio state, manage watchlists, record journal entries, review macro conditions, and generate briefs, all through a local database and a CLI with structured JSON output.

That makes pftui a strong foundation for:

- daily and weekly portfolio briefs
- automated monitoring and alerts
- scenario tracking
- research workflows
- multi-agent financial systems


### Core commands

```bash
pftui refresh --json               # Refresh prices, macro, sentiment, news
pftui brief --json                 # Full portfolio state
pftui summary --json               # Position-level breakdown
pftui macro --json                 # Macro, rates, commodities, sentiment
pftui watchlist --json             # Watched symbols and targets
pftui news --json                  # Aggregated financial news
pftui performance --json           # Returns across key periods
pftui drift --json                 # Allocation drift vs targets
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
pftui config list --json           # List all config fields
pftui config set brave_api_key <key>  # Set Brave Search API key
```
---

## 📦 Installation

**Recommended:** The install script is the canonical way to install AND upgrade pftui. Re-running it will detect your existing installation and update to the latest release version. Your data (SQLite database, config) is preserved across upgrades.

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

## Architecture

pftui is built as a four-layer intelligence stack. Each layer builds on the one below it, and the database sits at the centre as shared state for everything.

<img width="1376" height="768" alt="IMG_8031" src="https://github.com/user-attachments/assets/6d5e5832-e668-4b4e-8cbb-eb5d29be245f" />

---

### Data Aggregation Engine

One `pftui refresh` pulls from 10+ data sources, caches everything locally, and runs pre-processing on top of the raw data. By the time anything else reads from the database, the heavy numerical work is already done.

**What it collects:** Equity/crypto/commodity/forex prices across 84 symbols. CFTC Commitments of Traders positioning. COMEX gold and silver warehouse inventory. BLS economic data (CPI, NFP, unemployment, wages across 101 series). World Bank structural indicators for 8 economies. Polymarket prediction market odds. Crypto and traditional Fear and Greed indices. Economic calendar events. Financial news from 10+ RSS feeds and Brave Search. BTC on-chain data and ETF flows.

**What it computes:** RSI, MACD, SMA, and Bollinger Bands across all symbols. Rolling cross-asset correlation matrices. Market regime classification (risk-on, risk-off, transition) with confidence scoring. Daily change detection and threshold alerts. FX normalization for multi-currency portfolios. Prediction market probability shifts.

This pre-processing is important. The Analytics Engine does not re-derive technicals from raw price data. It reads pre-computed RSI from the database and asks the higher-order question: what does RSI 89 on oil mean given the current war scenario? The aggregation layer handles compute. The analytics layer handles interpretation.

**No API keys required** for core sources. Optional Brave Search API unlocks additional news, economic data, and research queries. See [docs/DATA-AGGREGATION.md](docs/DATA-AGGREGATION.md) for full source catalog and pipeline details.

```bash
pftui refresh     # Run the full pipeline
pftui status      # Check freshness per source
pftui doctor      # Connectivity diagnostics
```

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

### Database

The database is not a passive store. It is the shared state layer that every other layer reads from and writes to. The aggregation engine writes price caches, sentiment data, and economic indicators. The analytics engine writes scenarios, convictions, and regime classifications. The AI layer writes agent messages, daily notes, and predictions. Every layer's output becomes queryable input for every other layer.

SQLite by default, PostgreSQL for production. Your choice. Both are first-class backends with full feature parity.

**Day 1:** A snapshot of prices, sentiment, positioning, and economic data.

**Day 30:** A month of cross-asset price history, weekly COT shifts, and sentiment trends.

**Day 300:** A proprietary dataset covering daily OHLCV across every asset class, CFTC positioning history, COMEX inventory trends, sentiment cycles, and prediction market accuracy. The kind of data trading desks pay six figures for.

You own this data completely. No cloud sync. No third-party accounts. The longer you run pftui, the more valuable it becomes.

---

### Multi-timeframe Analytics Engine

The core differentiator. Four intelligence layers operating simultaneously across different time horizons. Each uses different data, updates at different frequencies, and produces different signals.

- **LOW (hours to days)** tracks what is happening right now. Prices, volatility, sentiment, regime classification, correlations, calendar events, and triggered alerts. Updated every refresh cycle. Signal type: tactical.
- **MEDIUM (weeks to months)** tracks which narratives are winning. Macro scenarios with probabilities, versioned thesis sections, conviction scores per asset, research questions with evidence, economic data, and user predictions with accuracy scoring. Updated daily. Signal type: directional.
- **HIGH (months to years)** tracks the structural forces reshaping markets. Multi-quarter trends like AI disruption, nuclear renaissance, and commodity supercycles. Each trend has a direction, evidence log, and per-asset impact mapping. Updated weekly or on significant evidence. Signal type: thematic.
- **MACRO (years to decades)** tracks where we are in the big cycle. Empire lifecycle analysis with power metrics across 8 dimensions, structural cycles with stage tracking, long-term outcome probabilities, and historical parallels with similarity scoring. Updated weekly. Signal type: structural.

Signals flow upward through the layers. A correlation break in LOW gets escalated to MEDIUM, which investigates whether it represents a scenario shift. A scenario shift in MEDIUM feeds evidence to a HIGH trend. Context flows downward. MACRO's assessment of the current empire cycle stage constrains how MEDIUM weights its scenarios, which constrains how LOW interprets short-term moves.

When all four layers agree on an asset, that is the highest conviction signal in the system. When they diverge, that divergence is the investigation worth doing.

```bash
pftui analytics summary                    # All four layers in one view
pftui analytics alignment --symbol GC=F    # Per-asset cross-timeframe consensus
pftui scenario add "Recession" --probability 30
pftui trends add "AI Disruption" --direction accelerating
pftui structural cycle-set "Big Debt Cycle" --stage 6
```

See the full documentation: [docs/ANALYTICS-ENGINE.md](docs/ANALYTICS-ENGINE.md)

---

### AI Agentic Layer

pftui is truly agent-native. It was written by AI, beta-tested by AI, and it has been built in public through a feedback loop of agents suggesting fixes and enhancements to other agents which then ship them. 

As such, pftui can be operated intuitively by your agent. The repository is fully populated with agentic docs that will assist your agent in bootstrapping a local pftui instance for you, integrating your own personal risk tolerances, time preferances, asset watchlists and portfolio holdings. pftui works best when driven by an always-running autonomous agent, though it can also be driven by session-based assistants.

Every feature in pftui has a CLI command with `--json` output. Agents use the same database, same commands, and same analysis frameworks that humans do. The result is genuine bidirectional intelligence where both operators contribute to a shared understanding.

**Bidirectional communication.** Your agent does not just read your data. It contributes to it. Agents update scenario probabilities, log evidence against research questions, set conviction scores, and write daily notes. You review what the agent wrote, adjust where you disagree, and the system incorporates both perspectives. The ongoing dialogue between human conviction and agent analysis is the most valuable output.

**Scheduled routines.** Morning briefs, market close summaries, weekly reviews, scenario analysis, feedback loop optimization. All cron-driven, all reading from and writing to the same database. Multiple agents can coordinate through `pftui agent-msg`, a structured message bus with priority levels, analytics layer tags, and acknowledgment tracking.

**Investor Perspectives Panel.** Feed your analytics engine data to sub-agents prompted as famous investors. Warren Buffett, Ray Dalio, Stanley Druckenmiller, Michael Burry, and 21 others. Each interprets the same data through a fundamentally different investment philosophy. The consensus tells you where conviction is strongest. The divergence tells you where the interesting questions are.

```bash
pftui refresh && pftui brief --json        # Agent gets full portfolio state
pftui scenario update "Stagflation" --probability 35
pftui conviction set GC=F --score 4 --notes "War + BRICS + CB buying"
pftui agent-msg send "Gold alignment: all 4 layers bullish" --from morning-agent --layer cross
```

See the full AI layer guide: [docs/AI-LAYER.md](docs/AI-LAYER.md) and agent operator guide: [AGENTS.md](AGENTS.md)

<table>
  <tr>
    <td align="center"><img width="400" alt="pftui portfolio overview" src="https://github.com/user-attachments/assets/8d3e2c8d-09aa-4fdf-9ef8-bed770a6ee12" /><br><sub>portfolio overview</sub></td>
    <td align="center"><img width="400" alt="pftui tx overview" src="https://github.com/user-attachments/assets/d77a5792-afbc-49c1-a76c-33c2a9d74965" /><br><sub>transactions</sub></td>
    <td align="center"><img width="400" alt="pftui economy overview" src="https://github.com/user-attachments/assets/97b9816c-4dd3-4660-b728-f194f56204a3" /><br><sub>economy</sub></td>
  </tr>
  <tr>
    <td align="center"><img width="400" alt="pftui analytics" src="https://github.com/user-attachments/assets/061b9ead-2f73-4e74-bf0b-682720ddafaa" /><br><sub>analytics</sub></td>
    <td align="center"><img width="400" alt="pftui web" src="https://github.com/user-attachments/assets/78043f32-c9a2-4ab4-b5fc-b01c7b9c23bd" /><br><sub>web dashboard</sub></td>
    <td align="center"><img width="400" alt="pftui web search" src="https://github.com/user-attachments/assets/314e4898-3514-4293-80f2-e4606d92f05e" /><br><sub>web search</sub></td>
  </tr>
  <tr>
    <td align="center"><img width="400" alt="pftui web economy" src="https://github.com/user-attachments/assets/cea4e33c-f60e-4286-ab66-c4c8b2e2eb5f" /><br><sub>web economy</sub></td>
    <td align="center"><img width="400" alt="pftui web asset detail" src="https://github.com/user-attachments/assets/cafe8ce9-7c5b-4876-8599-d8377058a5a6" /><br><sub>web asset detail</sub></td>
    <td align="center"><img width="400" alt="pftui cli" src="https://github.com/user-attachments/assets/ca929ee4-c999-4dd6-a796-d3442bf03048" /><br><sub>cli</sub></td>
  </tr>
</table>

---

## 🏗️ Built With

pftui is built in **Rust** as a fast, single-binary application.

It uses:

- **ratatui** for the terminal UI
- **SQLite** by default, with **PostgreSQL** support via `sqlx`
- **Actix-web** for the web dashboard
- **TradingView Widget** for interactive web charts

No Node required. No Python required. No Docker required.

---

## 🗺️ Roadmap

- Configurable homepage (portfolio-first vs watchlist-first)
- Full chart search for any symbol (`/` → chart → quick-add)
- Portfolio scenario engine (what-if modeling)
- Enhanced correlation and risk analytics
- Broader economic calendar and event intelligence

Full roadmap: [TODO.md](TODO.md) · Feature specs: [docs/ANALYTICS-SPEC.md](docs/ANALYTICS-SPEC.md)

---

## 📖 Documentation

| Document | Description |
|---|---|
| [PRODUCT-VISION.md](PRODUCT-VISION.md) | What pftui is, who it's for, and where it's going |
| [PRODUCT-PHILOSOPHY.md](PRODUCT-PHILOSOPHY.md) | Core beliefs, design decisions, and what pftui will never be |
| [AGENTS.md](AGENTS.md) | Agent operator guide, setup, workflows, integration patterns |
| [CLAUDE.md](CLAUDE.md) | Development guide for AI coding agents |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Code architecture, file map, line ranges |
| [docs/DATA-AGGREGATION.md](docs/DATA-AGGREGATION.md) | Data ingestion pipeline and source-unification model |
| [docs/ANALYTICS-SPEC.md](docs/ANALYTICS-SPEC.md) | Feature specifications |
| [docs/AI-LAYER.md](docs/AI-LAYER.md) | Agent operating model and multi-agent workflows |
| [docs/API-SOURCES.md](docs/API-SOURCES.md) | Free data source reference |
| [docs/MIGRATING.md](docs/MIGRATING.md) | Backend migration guide (SQLite/PostgreSQL) |
| [docs/BACKEND-PARITY.md](docs/BACKEND-PARITY.md) | Backend parity scope + verification/signoff runbook |
| [docs/DISTRIBUTION.md](docs/DISTRIBUTION.md) | Snap/AUR/Scoop/Homebrew distribution runbook |
| [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md) | Full keyboard shortcut reference |
| [docs/VISION.md](docs/VISION.md) | TUI design principles and quality bar |
| [WEB_DASHBOARD.md](WEB_DASHBOARD.md) | Web dashboard setup and API reference |
| [TODO.md](TODO.md) | Development backlog |
| [CHANGELOG.md](CHANGELOG.md) | Release history |

---

## 🤝 Contributing

Issues and PRs welcome. If you use pftui and have ideas, [open an issue](https://github.com/skylarsimoncelli/pftui/issues).

---

## 📝 License

MIT, do whatever you want with it.

---

<p align="center">
  <b>pftui</b>, portfolio intelligence for humans and agents.<br>
  Built with Rust 🦀 and OpenClaw 🦞
</p>
