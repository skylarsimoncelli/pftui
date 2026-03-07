# Philosophy

Why pftui exists and how we make decisions.

## The Problem

Financial data is trapped behind paywalls, bloated web apps, and platforms designed to make you trade more. Bloomberg costs $24k/year. TradingView nags you to upgrade. Robinhood gamifies your portfolio into a slot machine. Yahoo Finance wraps three numbers in seventeen ads.

Meanwhile, the terminal sits there — fast, private, infinitely composable — and nobody's built a serious financial tool for it since the 1980s.

## The Bet

A single Rust binary, a SQLite file, and a terminal emulator is all you need to track your portfolio, read the macro landscape, and make informed decisions. No accounts. No cloud. No subscriptions. No telemetry. Your data stays on your machine.

## Core Beliefs

### 1. Data should be trustworthy or absent

Never show wrong data. Never show stale data without saying so. If a source is down, show `---` — don't show yesterday's number without a stale indicator. Financial decisions are made on data quality. A tool that lies to you is worse than no tool at all.

### 2. Default output is the fullest

Every command shows everything it knows. Flags only filter and restrict — they never enable data that would otherwise be hidden. `pftui summary` shows the full picture. `pftui summary --category crypto` narrows it. There is no `--verbose` or `--full` flag. Full is the default.

### 3. One command per workflow

`pftui refresh` fetches everything. `pftui eod` gives you the full end-of-day picture. `pftui sentiment` combines COT positioning with Fear & Greed. Don't make the user run five commands and stitch together the output. Anticipate what they need and deliver it in one shot.

### 4. Terminal-native means terminal-native

No Electron. No web views in disguise. Braille characters for charts. Block elements for volume. Real cursor movement, real keyboard events, real resize handling. If your terminal can run `htop`, it can run `pftui`. The constraints of the terminal are features, not limitations — they force information density.

### 5. Agents are first-class users

Every CLI command supports `--json`. Structured output isn't an afterthought — it's how AI agents, scripts, and pipelines consume your portfolio data. A human runs `pftui macro`. An agent runs `pftui macro --json` and makes decisions. Both are equally supported.

### 6. Precision is non-negotiable

`rust_decimal` for all money. No floats. No rounding errors that compound over time. If you bought 0.00142857 BTC at $69,420.50, that's exactly what the database stores and exactly what the UI shows. Financial software that rounds is financial software that lies.

### 7. Privacy by default

Percentage mode shows portfolio composition without revealing dollar amounts. The `p` key instantly hides monetary values. No telemetry, no analytics, no phone-home. The binary doesn't even make network requests until you explicitly run `refresh`. Your portfolio is nobody's business.

## Design Decisions That Follow

These aren't arbitrary rules — they fall out of the beliefs above:

- **SQLite, not a server** — your data is a file you can backup, move, or delete
- **Vim keybindings** — the terminal's native language, not a custom invention
- **All themes hand-tuned** — aesthetics aren't optional; you stare at this daily
- **Graceful degradation** — show cached data instantly, update live data in the background
- **No trading** — read-only by design; this is an intelligence tool, not an execution platform
- **Free data sources only** — if it requires an API key or subscription, it's optional, never required

## What We Don't Build

- Order execution or trading
- Social features or sharing
- Cloud sync or accounts
- Gamification (streaks, achievements, confetti)
- Notifications designed to increase engagement
- Features that require paid API access to function

## The Standard

Before shipping a feature, ask:

1. **Would I trust this data to make a real decision?** If not, fix it or remove it.
2. **Does this work with `--json`?** If not, agents can't use it.
3. **Does this work in all themes?** If not, someone's staring at broken colors daily.
4. **Is this one command or three?** If three, combine them.
5. **Does this respect privacy mode?** If not, someone's portfolio just leaked over their shoulder.
