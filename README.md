<p align="center">
  <b>pftui</b><br>
  portfolio intelligence in your terminal<br>
  <a href="https://pftui.com">pftui.com</a>
</p>

<p align="center">
  <a href="https://crates.io/crates/pftui"><img src="https://img.shields.io/crates/v/pftui.svg" alt="crates.io"></a>
  <a href="https://github.com/skylarsimoncelli/pftui/releases"><img src="https://img.shields.io/github/v/release/skylarsimoncelli/pftui" alt="release"></a>
  <a href="https://github.com/skylarsimoncelli/pftui/blob/master/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="license"></a>
</p>

---

**pftui** is a local-first portfolio intelligence system with a multi-timeframe analytics engine.

It aggregates **19+ data sources** (prices, CFTC positioning, COMEX inventory, BLS economic data, FedWatch probabilities, prediction markets, ETF flows, on-chain metrics), computes cross-asset correlations and regime classification, and runs four analytics layers spanning intraday volatility to decade-long structural cycles to produce situation awareness, catalyst ranking, impact analysis, and opportunity scoring. Terminal UI. Web dashboard. **100+ CLI commands** with structured JSON output. Your AI agent operates the same system you do.

---

## Why pftui

- **Multi-timeframe analytics engine**: four intelligence layers (LOW/MEDIUM/HIGH/MACRO) classify market regime, rank catalysts by portfolio impact, detect cross-timeframe alignment and divergence, and score opportunities with evidence chains. No other CLI tool does this.
- **19+ data sources, one command**: `pftui data refresh` pulls prices (84 symbols), CFTC COT positioning, COMEX warehouse inventory, BLS economic data (101 series), FedWatch rate probabilities, oil term structure, sovereign gold/BTC holdings, prediction market odds, Fear & Greed indices, ETF flows, on-chain metrics, and financial news. Cached locally. No API keys required.
- **Falsifiable predictions with scoring**: track predictions, score outcomes, measure your hit rate by conviction level and timeframe. The system tells you where you're calibrated and where you're overconfident.
- **Local-first, zero-config**: SQLite on your machine. No cloud account. No API keys for core sources. Install and go. PostgreSQL for production deployments.
- **100+ CLI commands with `--json`**: every feature is a composable CLI primitive. Agents operate the same system humans do. The recommended setup is "tell your agent to install pftui and set it up with you."
- **Broker sync**: connect Trading212, IBKR, Binance, Kraken, Coinbase, or Crypto.com and pull positions automatically

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
pftui system web                          # Start on localhost:8080 with auth
pftui system web --port 3000 --bind 0.0.0.0   # Custom port, remote access
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
# Refresh all 19+ data sources in one call
pftui data refresh --json

# Situation awareness: what matters right now
pftui analytics situation --json
# Returns: regime classification, watch-now items with severity,
# portfolio impacts, alignment state, alert status

# What changed since last check
pftui analytics deltas --json
# Returns: change radar with severity scoring across price moves,
# regime shifts, alert triggers, scenario probability changes

# Ranked upcoming catalysts with portfolio relevance scoring
pftui analytics catalysts --json
# Returns: calendar events, linked scenarios, affected assets,
# countdown buckets, composite importance score

# Impact analysis: what matters to YOUR book
pftui analytics impact --json
# Returns: per-position consensus (bullish/bearish/mixed), evidence
# chains linking conviction + trend impacts + technical signals
# + scenario probabilities into a single ranked view

# High-alignment ideas you don't currently hold
pftui analytics opportunities --json
# Returns: non-held assets ranked by cross-timeframe alignment,
# conviction, trend linkage, and catalyst proximity

# Cross-timeframe synthesis: where do the layers agree?
pftui analytics synthesis --json
# Returns: strongest alignments, highest-confidence divergences,
# layer-by-layer bias per asset (LOW/MEDIUM/HIGH/MACRO)

# Full asset intelligence dossier
pftui analytics asset BTC --json
# Returns: price, technicals, correlations, conviction history,
# scenario links, trend impacts, alerts, alignment, everything
# the system knows about one asset in one blob

# Portfolio state
pftui portfolio brief --json
pftui portfolio drift --json
pftui portfolio performance --json
```

### Portfolio Management

```bash
pftui portfolio transaction add --symbol AAPL --category equity --tx-type buy \
  --quantity 100 --price 175.50 --date 2026-03-01 --notes "Earnings dip"
pftui portfolio transaction remove 42
pftui portfolio set-cash USD 50000
pftui portfolio watchlist add TSLA --target 300
pftui portfolio watchlist remove TSLA
pftui portfolio target set AAPL --target 15  # Target allocation %
pftui portfolio rebalance --json             # Suggested trades to hit targets
pftui analytics alerts add "BTC above 100000"
pftui system config list --json           # List all config fields
pftui system config set brave_api_key <key>  # Set Brave Search API key
```
---

## 📦 Installation

**Recommended:** use the install script. Re-running it upgrades pftui while preserving your local data and config.

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

## In Practice

A quick look at pftui across the terminal UI, web dashboard, analytics views, and CLI.

<table>
  <tr>
    <td align="center"><img width="400" alt="pftui portfolio overview" src="https://github.com/user-attachments/assets/7cc1b734-d570-4309-ba0f-1c2554775245" /><br><sub>portfolio overview</sub></td>
    <td align="center"><img width="400" alt="pftui tx overview" src="https://github.com/user-attachments/assets/a5a6ebdc-8f7d-4585-a081-d16a64926593" /><br><sub>transactions</sub></td>
    <td align="center"><img width="400" alt="pftui market economy overview" src="https://github.com/user-attachments/assets/3c201a5f-44d2-46a7-aa87-a29af37bcb4d" /><br><sub>economy</sub></td>
  </tr>
  <tr>
    <td align="center"><img width="400" alt="pftui analytics" src="https://github.com/user-attachments/assets/2a2fb8d9-baad-4f29-8613-6ddcc2786c85" /><br><sub>analytics</sub></td>
    <td align="center"><img width="400" alt="pftui system web" src="https://github.com/user-attachments/assets/78043f32-c9a2-4ab4-b5fc-b01c7b9c23bd" /><br><sub>web dashboard</sub></td>
    <td align="center"><img width="400" alt="pftui system web search" src="https://github.com/user-attachments/assets/314e4898-3514-4293-80f2-e4606d92f05e" /><br><sub>web search</sub></td>
  </tr>
</table>

---

## Architecture

pftui is built as a four-layer intelligence stack. Each layer builds on the one below it, and the database sits at the centre as shared state for everything.

<img width="1376" height="768" alt="IMG_8031" src="https://github.com/user-attachments/assets/6d5e5832-e668-4b4e-8cbb-eb5d29be245f" />

---

### Data Aggregation Engine

One `pftui data refresh` pulls from 19+ data sources, caches everything locally, and runs pre-processing on top of the raw data. By the time anything else reads from the database, the heavy numerical work is already done.

**What it collects:** Equity/crypto/commodity/forex prices across 84 symbols. CFTC Commitments of Traders positioning. COMEX gold and silver warehouse inventory. BLS economic data (CPI, NFP, unemployment, wages across 101 series). World Bank structural indicators for 8 economies. Polymarket prediction market odds. Crypto and traditional Fear and Greed indices. Economic calendar events. Financial news from 10+ RSS feeds and Brave Search. BTC on-chain data and ETF flows.

**What it computes:** RSI, MACD, SMA, and Bollinger Bands across all symbols. Rolling cross-asset correlation matrices. Market regime classification (risk-on, risk-off, transition) with confidence scoring. Daily change detection and threshold alerts. FX normalization for multi-currency portfolios. Prediction market probability shifts.

This pre-processing is important. The Analytics Engine does not re-derive technicals from raw price data. It reads pre-computed RSI from the database and asks the higher-order question: what does RSI 89 on oil mean given the current war scenario? The aggregation layer handles compute. The analytics layer handles interpretation.

**No API keys required** for core sources. Optional Brave Search API unlocks additional news, economic data, and research queries. See [docs/DATA-AGGREGATION.md](docs/DATA-AGGREGATION.md) for full source catalog and pipeline details.

```bash
pftui data refresh     # Run the full pipeline
pftui data status      # Check freshness per source
pftui system doctor      # Connectivity diagnostics
```

Every data source works out of the box with no API keys:

| Source | Data | Update Cadence |
|---|---|---|
| Yahoo Finance | Equities, ETFs, forex, crypto, commodities (84 symbols) | Real-time |
| CoinGecko | Crypto prices, market cap, volume | Real-time |
| CME FedWatch | Fed funds futures-implied rate probabilities | Daily |
| CME/Yahoo | Oil futures term structure (contango/backwardation, war premium) | Daily |
| CFTC Socrata | Commitments of Traders (COT) positioning | Weekly |
| CME Group | COMEX gold/silver warehouse inventory | Daily |
| WGC / BitcoinTreasuries | Central bank gold reserves, government BTC holdings | Quarterly/Daily |
| BLS API v1 | CPI, unemployment, NFP, wages (101 series) | Monthly |
| World Bank | GDP, debt/GDP, reserves for 8 economies (160 indicators) | Quarterly |
| Polymarket | Prediction market probabilities | 15-min |
| Alternative.me | Crypto Fear & Greed Index | Daily |
| Blockchair | BTC on-chain data, ETF flows | Real-time |
| EIA | Weekly crude oil inventory, SPR levels | Weekly |
| Yahoo Options | Options chains for equity symbols | Real-time |
| RSS Feeds | Reuters, CoinDesk, Bloomberg, Kitco, CNBC (10+ feeds) | 10-min |

Optional API keys unlock additional sources (Finnhub, FRED, Alpha Vantage). See [docs/API-SOURCES.md](docs/API-SOURCES.md).

---

### Database

The database is not a passive store. It is the shared state layer that every other layer reads from and writes to. The aggregation engine writes price caches, sentiment data, technical state, and economic indicators. The analytics engine writes scenarios, convictions, regime classifications, situation snapshots, deltas, and narrative state. The AI layer writes agent messages, daily notes, and predictions. Every layer's output becomes queryable input for every other layer.

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

On top of those four layers, pftui defines a set of shared analytics contracts across CLI, web, mobile, and agent workflows:

- `analytics situation`: what matters now
- `analytics deltas`: what changed
- `analytics catalysts`: what is coming next
- `analytics impact`: why it matters to the current book
- `analytics opportunities`: what high-alignment ideas are outside the book
- `analytics synthesis`: where timeframes agree or conflict
- `analytics narrative`: structured recap and analytical memory

These shared analytics contracts keep the architecture coherent across every surface: the same market situation, change radar, catalyst pressure, portfolio impact, cross-timeframe synthesis, and analytical memory appear wherever you access pftui.

```bash
pftui analytics correlations compute --store --period 30d      # Compute and persist live correlations
pftui analytics correlations history BTC SPY --period 30d --limit 30 --json
pftui analytics macro regime current --json                    # Current automated regime classification
pftui analytics macro regime transitions --limit 20 --json     # Recent regime change points
pftui analytics macro --json                                   # Long-cycle macro dashboard
pftui analytics macro outcomes --json                          # Structural outcome probabilities
pftui analytics trends dashboard --json                        # Active high-timeframe trends
pftui analytics trends impact add --trend "AI Disruption" --symbol NVDA --impact bullish
pftui analytics summary --json                                 # Unified four-layer analytics snapshot
pftui analytics situation --json                               # Canonical Situation Room payload
pftui analytics deltas --json                                  # What changed: last refresh, close, 24h, 7d
pftui analytics catalysts --json                               # Ranked upcoming catalysts with countdowns
pftui analytics impact --json                                  # What matters to your current book
pftui analytics opportunities --json                           # High-alignment non-held opportunities
pftui analytics synthesis --json                               # Alignment, divergence, constraints, watch tomorrow
pftui analytics alignment --symbol GC=F --json                 # Per-asset cross-timeframe consensus
pftui analytics divergence --json                              # Cross-layer disagreement table
pftui analytics digest --from low-agent --json                 # Role-aware handoff payload
pftui analytics recap --date yesterday --json                  # Chronological recap for a given day
pftui analytics narrative --json                               # Structured analytical memory and recap state
pftui analytics gaps --json                                    # Freshness / missing-table checks
pftui analytics signals --json                                 # All signals
pftui analytics signals --source technical --json              # Technical signals only
pftui analytics signals --source timeframe --json              # Cross-timeframe signals only
pftui analytics signals --source technical --symbol BTC-USD --json
pftui analytics technicals --symbol BTC-USD --json             # Persisted technical snapshot(s)
```

#### What the analytics engine produces

**Situation awareness**: the canonical "what matters now" payload:
```json
{
  "headline": "Alignment",
  "subtitle": "bearish cross-timeframe alignment (3 / 4 layers agree)",
  "watch_now": [
    {"title": "3 live alerts need triage", "severity": "critical"},
    {"title": "Regime: Risk-off", "value": "75%"}
  ],
  "portfolio_impacts": [
    {"title": "GC=F", "detail": "Gold • 23.3% allocation", "value": "2.73"},
    {"title": "BTC", "detail": "Bitcoin • 20.1% allocation", "value": "-1.20"}
  ]
}
```

**Impact analysis**: what matters to your specific book:
```json
{
  "exposures": [{
    "symbol": "GC=F", "consensus": "bullish", "score": 142,
    "evidence_chain": [
      "2 bull / 0 bear layers (63%). Conviction +4.",
      "Trend AI Displacement is bullish via safe haven.",
      "Scenario War Escalation at 85% probability."
    ]
  }]
}
```

**Cross-timeframe synthesis**: where the layers agree and disagree:
```json
{
  "strongest_alignment": [
    {"symbol": "GOOG", "low": "bear", "medium": "bear", "high": "bear", "consensus": "BEARISH"}
  ],
  "highest_confidence_divergence": [
    {"symbol": "BTC", "low": "bear", "medium": "bull", "high": "bull", "dominant_side": "bull"}
  ]
}
```

See the full documentation: [docs/ANALYTICS-ENGINE.md](docs/ANALYTICS-ENGINE.md)

---

### Research Journal

The journal turns portfolio management from "I think gold will go up" into a structured, accountable research practice.

**Predictions with scoring.** Make a call, assign conviction (low/medium/high) and timeframe, then score the outcome. pftui tracks your accuracy across the whole system, by conviction level, by asset class, and by timeframe.

```bash
pftui journal prediction add "Gold outperforms equities into Q2" \
  --symbol GC=F --conviction high --timeframe medium
pftui journal prediction score --id 42 --outcome correct \
  --lesson "Rates rollover mattered more than expected"
pftui journal prediction stats --json
```

```json
{
  "total": 165, "scored": 102, "correct": 46, "hit_rate_pct": 45.1,
  "by_conviction": {
    "high": {"scored": 29, "correct": 14, "hit_rate_pct": 48.3},
    "medium": {"scored": 64, "correct": 28, "hit_rate_pct": 43.8}
  }
}
```

**Scenarios with probability tracking.** Define macro scenarios, assign probabilities that evolve with evidence, and link signals that move the needle.

```bash
pftui journal scenario add "Stagflation" --probability 35
pftui journal scenario signal add "Oil above $90 sustained" --scenario "Stagflation"
pftui journal scenario update "Stagflation" --probability 60 --notes "NFP collapse + sticky CPI"
```

**Conviction scores.** Track your conviction on any asset over time (-5 to +5). The analytics engine reads these scores and incorporates them into impact analysis and alignment detection.

```bash
pftui journal conviction set GC=F --score 4 --notes "War premium + BRICS"
pftui journal conviction set BTC --score 2 --notes "ETF flows strong but macro headwinds"
```

---

### AI Agentic Layer

pftui is truly agent-native. It was written by AI, beta-tested by AI, and it has been built in public through a feedback loop of agents suggesting fixes and enhancements to other agents which then ship them. 

As such, pftui can be operated intuitively by your agent. The repository is fully populated with agentic docs that will assist your agent in bootstrapping a local pftui instance for you, integrating your own personal risk tolerances, time preferances, asset watchlists and portfolio holdings. pftui works best when driven by an always-running autonomous agent, though it can also be driven by session-based assistants.

Every feature in pftui has a CLI command with `--json` output. Agents use the same database, same commands, and same analysis frameworks that humans do. The result is genuine bidirectional intelligence where both operators contribute to a shared understanding.

**Bidirectional communication.** Your agent does not just read your data. It contributes to it. Agents update scenario probabilities, log evidence against research questions, set conviction scores, and write daily notes. You review what the agent wrote, adjust where you disagree, and the system incorporates both perspectives. The ongoing dialogue between human conviction and agent analysis is the most valuable output.

**Scheduled routines.** Morning briefs, market close summaries, weekly reviews, scenario analysis, feedback loop optimization. All cron-driven, all reading from and writing to the same database. Multiple agents can coordinate through `pftui agent message`, a structured message bus with priority levels, analytics layer tags, and acknowledgment tracking.

**Investor Perspectives Panel.** Feed your analytics engine data to sub-agents prompted as famous investors. Warren Buffett, Ray Dalio, Stanley Druckenmiller, Michael Burry, and 21 others. Each interprets the same data through a fundamentally different investment philosophy. The consensus tells you where conviction is strongest. The divergence tells you where the interesting questions are.

```bash
pftui data refresh && pftui portfolio brief --json        # Agent gets full portfolio state
pftui journal entry add "Gold thesis validated by CPI" --tag macro
pftui journal entry list --json
pftui journal entry search "gold thesis" --json
pftui journal scenario add "Recession" --probability 30
pftui journal scenario update "Stagflation" --probability 35 --notes "Sticky inflation + growth slowdown"
pftui journal scenario signal add "Yield curve reinversion" --scenario "Recession"
pftui journal scenario history "Stagflation" --limit 20 --json
pftui journal prediction add "Gold outperforms equities into Q2" --symbol GC=F --conviction high --timeframe medium
pftui journal prediction score --id 42 --outcome correct --lesson "Rates rollovers mattered more than expected"
pftui journal prediction stats --json
pftui journal prediction scorecard --date yesterday --timeframe low --json
pftui journal notes add "Fed hold keeps real-rate pressure elevated" --section market
pftui journal notes search "real-rate pressure" --since 2026-03-01 --json
pftui agent message send "Gold alignment: all 4 layers bullish" --from morning-agent --layer cross
```

See the full AI layer guide: [docs/AI-LAYER.md](docs/AI-LAYER.md) and agent operator guide: [AGENTS.md](AGENTS.md)

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
