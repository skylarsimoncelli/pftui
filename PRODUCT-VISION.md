# Product Vision

## One Sentence

pftui is the financial intelligence platform where humans and AI agents collaborate on portfolio management — centralising all market data, news, and economic intelligence into one local-first tool that either can operate.

## The Problem

Financial intelligence is scattered. You check Yahoo Finance for prices, TradingView for charts, Reuters for news, FRED for economic data, Twitter for sentiment, CoinGlass for ETF flows, CME for COMEX data, Polymarket for prediction markets. Your AI agent does the same thing — running dozens of web searches every morning to assemble a picture of what happened overnight.

Both of you are doing redundant work. Both of you are context-switching between sources. Neither of you has a single place where all the data lives, persists, and compounds over time.

## The Solution

pftui centralises everything into one tool with three interfaces:

**For the human:** A terminal UI and web dashboard that show your portfolio, market data, charts, macro indicators, news, sentiment, and economic calendar — all in one screen. No browser tabs. No subscriptions. No API keys required.

**For the agent:** A CLI with structured JSON output for every feature, backed by a local SQLite database. The agent can refresh all data sources in one command, get a complete portfolio snapshot in one JSON blob, and research any financial question without leaving the tool.

**For both:** A shared database that serves as the single source of truth. The agent writes analysis, the human reads it. The human makes decisions, the agent tracks them. Both operate on the same data, in the same tool, building institutional knowledge that compounds over time.

## Who This Is For

**The primary user is a human who has an AI agent.** Not a day trader. Not a quant. A person who cares about their financial future, has opinions about markets, and wants an AI partner to help them stay informed, test their thinking, and catch what they'd miss.

The human might be:
- A software engineer with a portfolio they want to manage more seriously
- A macro investor who follows geopolitics, central banks, and commodities
- A crypto holder who wants context beyond coin prices
- Anyone who'd use a Bloomberg Terminal if it didn't cost $24k/year

The agent might be:
- Claude Code, Codex, or OpenClaw running on their machine
- A cron job delivering morning briefs
- A persistent agent with its own thesis, scenarios, and accuracy tracking
- Any AI assistant that can run CLI commands

## The Experience

### Getting Started

A new user installs pftui and tells their AI agent: "Set up pftui for me." The agent reads AGENTS.md, asks the human about their holdings, risk tolerance, and market views, populates the database, configures the watchlist, and delivers the first brief. Setup takes 15-20 minutes of conversation.

### Daily Operation

The agent runs `pftui refresh` every morning, pulls all data sources (prices, news, macro, sentiment, predictions), assembles a brief, and delivers it. The human reads it on their phone (web dashboard) or terminal (TUI). If something big happened overnight — a 5% BTC move, an oil shock, a Fed surprise — the agent flags it immediately.

The human opens the TUI when they want to dig deeper. Charts, technicals, correlation data, economic calendar, prediction markets — all keyboard-navigable, all instant. When they have a question ("what's the COT positioning on gold?"), they ask the agent, who answers from pftui data without needing to web search.

### Long-Term

Over weeks and months, the system builds institutional knowledge. The agent's thesis evolves. Its accuracy record grows. The journal tracks every decision and outcome. The human's views are catalogued and referenced. Scenarios are tracked and probabilities updated. The tool gets smarter because the data compounds.

## What Makes pftui Different

1. **Agent-first, not agent-compatible.** The CLI and data model aren't afterthoughts — they're the primary interface for the agent. Every feature ships with `--json`. The database schema is documented. The agent operator guide (AGENTS.md) is as thorough as the user manual.

2. **Data centralisation.** One `pftui refresh` fetches prices, news, macro, sentiment, predictions, COT positioning, COMEX inventory, economic releases, and more. The agent doesn't need 10 web searches — it needs one command.

3. **Local-first, zero-config.** SQLite database on your machine. No cloud account. No API keys required for core functionality. Optional Brave Search API key for enhanced intelligence. Install and go.

4. **Dual interface for dual operators.** The TUI and web dashboard are designed for human cognition — visual, scannable, information-dense. The CLI is designed for agent consumption — structured, parseable, composable. Same data, different presentation.

5. **Institutional memory.** The database persists. The journal persists. The agent's analysis files persist. Every day builds on the last. This is what separates a tool from a system.

## The Roadmap

### Now (v0.4)
- Full TUI with 7 views, 11 themes, braille charts
- Web dashboard with TradingView charts
- 30+ CLI commands with JSON output
- 10+ free data sources (Yahoo, CoinGecko, Polymarket, CFTC, BLS, RSS, etc.)
- Macro dashboard with technicals (RSI, MACD, SMA)
- COT positioning, COMEX inventory, economic calendar
- Multi-currency FX support
- Comprehensive agent operator guide

### Next (v0.5)
- Native Brave Search API integration — one API replaces fragile scrapers
- `pftui research` command for arbitrary financial queries
- `brief --agent` as the single intelligence blob
- Configurable homepage (portfolio-first vs watchlist-first)
- Full chart search for any symbol
- Portfolio scenario engine (what-if modeling)

### Future (v1.0)
- PostgreSQL backend for multi-agent deployments
- Correlation matrix and risk analytics
- Dividend tracking and tax lot management
- Plugin system for custom data sources
- Multi-portfolio support
- Real-time WebSocket price updates

## Success Metrics

pftui succeeds when:
- A human installs it, tells their agent to set it up, and gets their first brief within 30 minutes
- The agent's morning routine is `pftui refresh && pftui brief --agent --json` instead of 15 web searches
- The human checks pftui (TUI or web) instead of opening 5 browser tabs
- The system gets measurably smarter over time — better predictions, fewer missed moves, more calibrated advice
- Someone says "I cancelled my Bloomberg Terminal subscription"
