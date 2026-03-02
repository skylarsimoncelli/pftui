# TODO — pftui

> Pick the highest-priority unclaimed item. Remove it from this file when done.
> Each item is scoped to ~1 hour. If it's bigger, split it. Update CHANGELOG.md when done.

## P1 — Header & Status Bar Enhancements


## P1 — Regime Intelligence (Sentinel Proposed)

- [ ] **Top Movers section on homepage** — Group assets by category (crypto, equity, commodity, forex) and rank by daily/weekly % change. Display as a compact panel:
  ```
  🔥 TOP MOVERS
  Crypto    BTC +3.5%   ETH +2.1%   SOL -4.2%
  Equity    PLTR +7.8%  NFLX +13%   NVDA -4.2%
  Commodity Gold +2.5%  Oil +7.3%   Silver +0.6%
  Forex     GBP -0.8%  EUR -0.3%   JPY +0.4%
  ```
  Toggleable 1D/1W/1M. Uses existing price data. Files: new `src/tui/widgets/top_movers.rs`, integrate into `src/tui/views/positions.rs` or new homepage layout.

- [ ] **Risk-On/Risk-Off Regime Health Bar** — Composite regime score from 9 cross-asset signals (all freely available via Yahoo Finance): VIX level, VIX 5D direction, 10Y yield direction, 2Y-10Y spread, DXY direction, Gold vs S&P 5D ratio, BTC/SPX correlation, HY credit spread (HYG/LQD ratio), Copper/Gold ratio. Each signal +1 (risk-on) or -1 (risk-off). Display as a gauge:
  ```
  ⚡ REGIME: RISK-OFF ████████░░ -6/9
     VIX 23.7↑  10Y 3.98↓  DXY 97.9↑  Cu/Au↓  Gold>SPX
  ```
  Show individual signal breakdown below the bar. Files: new `src/regime/mod.rs` (signal computation), new `src/tui/widgets/regime_bar.rs`, integrate into homepage. Needs: rolling 5D/30D price data for direction and correlation calculations.

- [ ] **Regime Asset Suggestions** — Based on regime score, show assets that historically perform well/poorly in current regime. NOT "buy this" — framed as regime context:
  ```
  📡 REGIME ASSETS
  Strong in current regime: Gold, Silver, Treasuries, USD, Utilities
  Weak in current regime: Growth stocks, Crypto, High-yield, Copper
  Transitioning: Oil (geopolitical override), Uranium (structural)
  ```
  Portfolio-aware: flag if user's holdings are well/poorly positioned for current regime ("Your portfolio is 33% metals — well positioned for risk-off"). Files: new `src/regime/suggestions.rs`, new `src/tui/widgets/regime_assets.rs`. Depends on regime score from health bar.

## P1 — CLI Enhancements (Feedback)

- [ ] **[Feedback] Native multi-currency with live FX conversion** — Store non-USD currencies in their native denomination (e.g., GBP as GBP, not as USD equivalent). Convert to primary currency using live FX rates in summary/TUI. Show FX rate and flag currency risk. This is a larger effort — may need to split into sub-tasks. Files: `src/models/position.rs`, `src/price/mod.rs` (FX rate fetching), `src/commands/summary.rs`, `src/tui/widgets/header.rs`.
- [ ] **[Feedback] Add `pftui snapshot` / `pftui render` command** — Dump the TUI view as ANSI text to stdout, enabling agents to review the visual layout without running interactively. Files: new `src/commands/snapshot.rs`, `src/cli.rs` (use ratatui's `TestBackend` or similar to render to string).

## P2 — Chart Visual Enhancements

- [ ] **Add chart area fill with gradient** — Instead of just braille dots for the line, fill the area below the line with a fading gradient using BLOCK characters at very low intensity (10-20% alpha via dark versions of chart colors). Creates a "filled area chart" effect common in financial dashboards. Files: `src/tui/widgets/price_chart.rs` (render_braille_chart area fill pass). Test: verify fill doesn't exceed chart line position.
- [ ] **Add Bollinger Bands overlay** — Compute 20-period SMA ± 2 standard deviations. Render as faint dotted braille lines above and below the SMA(20). When price touches a band, highlight the touch point. Shows volatility and overbought/oversold conditions. Files: `src/tui/widgets/price_chart.rs` (compute_bollinger, overlay rendering). Test: test band computation with known data.

## P2 — Layout & Visual Polish

- [ ] **Add Unicode box-drawing panel separators** — Replace the default ratatui `Rounded` border type with custom double-line top (═══) and single-line sides (│). Use `╔═══╗` style for the active panel and `┌───┐` for inactive. Gives a more premium, Bloomberg-like feel. Files: `src/tui/views/positions.rs`, `src/tui/widgets/price_chart.rs`, `src/tui/widgets/allocation_bars.rs`, `src/tui/widgets/portfolio_sparkline.rs`. Test: visual verification only.
- [ ] **Add shadow effect on popups** — When the detail popup or help overlay renders, draw a 1-cell shadow on the right and bottom edges using `surface_0` with slight offset. Creates a floating/elevated look. Files: `src/tui/views/position_detail.rs`, `src/tui/views/help.rs`. Test: verify shadow doesn't exceed terminal bounds.

- [ ] **Add ultra-wide layout (160+ columns)** — When terminal is very wide, show a third column: market context panel with major indices and the portfolio sparkline below the positions table, with sidebar remaining as the chart panel. Three-column layout: 45% positions / 25% market context / 30% chart. Files: `src/tui/ui.rs` (new layout branch), new `src/tui/widgets/market_context.rs`. Test: test layout thresholds.

## P2 — Sidebar & Sparkline Enhancements



## P2 — Micro-Interactions & Feedback

- [ ] **Add loading skeleton for empty states** — When a view is loading data, show shimmer/skeleton placeholder rows instead of "Waiting for data...". Render 5-6 rows of `░░░░░░` block characters in `text_muted` with a wave animation (phase offset per row). Makes loading feel fast and intentional. Files: `src/tui/views/positions.rs`, `src/tui/views/markets.rs`, `src/tui/views/economy.rs`. Test: verify skeleton row count matches expected.

## P2 — Theme & Color Enhancements



## P2 — Data & Infrastructure

- [ ] **Add news feed integration** — Fetch financial news from a free RSS/API source (e.g., Yahoo Finance RSS, Finnhub free tier). Display as a scrollable list: timestamp, headline, source. Per-asset filtering. Files: new `src/news/` module, new `src/tui/views/news.rs`. Research: find best free news API that works without API key.
- [ ] **Add FRED economic data** — FRED API (free with API key) for treasury yields, CPI, unemployment, Fed funds rate. Store in new DB table. Cache aggressively (economic data updates daily at most). Files: new `src/data/fred.rs`, `src/db/economic_cache.rs`.
- [ ] **Add candlestick chart variant** — OHLC candlestick rendering using braille/block characters. Green body for close > open, red for close < open. Wicks as thin lines. Requires OHLC data in HistoryRecord. Files: `src/models/price.rs`, `src/price/yahoo.rs`, `src/tui/widgets/price_chart.rs`.


## P2 — Scenario & Analytics (Feedback)

- [ ] **[Feedback] Add `--what-if` flag to summary** — `pftui summary --what-if GC=F:5500,BTC:55000` to model hypothetical price scenarios. Compute portfolio value and allocation under hypothetical prices. Transformative for scenario planning. Files: `src/commands/summary.rs` (parse what-if pairs, override cached prices for computation).
- [ ] **[Feedback] Add historical price snapshots** — `pftui history --date 2026-02-28` to show portfolio value and positions as of a past date using cached price history. Files: new `src/commands/history.rs`, `src/cli.rs`, `src/db/price_history.rs`.

## P3 — Future

- [ ] **Portfolio analytics** — Sharpe ratio, max drawdown, volatility metrics, benchmark comparison
- [ ] **Dividend tracking** — Track dividend payments, show yield, ex-dates
- [ ] **Correlation matrix** — Visual correlation grid between portfolio positions
- [ ] **Multi-portfolio support** — Multiple named portfolios with switching
- [ ] **[Feedback] Price alerts** — Configurable threshold alerts with terminal notification. Feedback requests: `pftui alert GC=F above 5500` or `pftui alert GBPUSD below 1.30`. Both CLI and TUI integration. Bumped from P3 per tester request.
- [ ] **Custom keybinding config** — User-configurable keybindings in config.toml
- [ ] **Sector heatmap** — Treemap-style sector/industry performance view
- [ ] **Options chains** — Options display if a free data source exists


## P1 — Distribution & CI (Owner Priority)

> Name "pftui" is unclaimed on ALL major package managers. Prioritize crates.io and Homebrew first (covers 90% of terminal users), then expand.

- [ ] **Set up GitHub Actions CI** — Workflow for: `cargo test`, `cargo clippy`, `cargo build --release` on push/PR. Matrix: ubuntu-latest, macos-latest. Cache cargo registry + target dir. Files: new `.github/workflows/ci.yml`.
- [ ] **GitHub Releases with prebuilt binaries** — CI workflow that triggers on git tag (`v*`). Builds release binaries for linux-x86_64, linux-aarch64, macos-x86_64, macos-aarch64. Uploads as GitHub Release assets with checksums. Files: new `.github/workflows/release.yml`.
- [ ] **Homebrew formula** — Create a Homebrew tap (`homebrew-tap` repo) with a formula that downloads the GitHub Release binary for macOS. Auto-update formula on new releases via CI. `brew install skylarsimoncelli/tap/pftui`. Files: new repo `homebrew-tap`, formula `Formula/pftui.rb`, update release workflow to trigger formula bump.
- [ ] **Add install instructions to README** — Once releases and Homebrew are live, update README with: `brew install`, direct binary download, and `cargo install pftui` options. Files: `docs/README.md`.
- [ ] **Publish to crates.io** — `cargo publish` via CI on release tag. Enables `cargo install pftui` for Rust users. Add `description`, `license`, `repository`, `homepage`, `keywords`, `categories` to Cargo.toml. Files: `Cargo.toml`, release workflow.
- [ ] **AUR package** — Create an Arch Linux AUR package (`pftui-bin` for prebuilt, `pftui` for source build). PKGBUILD downloads from GitHub Releases. Files: new AUR repo, `PKGBUILD`.
- [ ] **Nix package** — Add a `flake.nix` for Nix/NixOS users. `nix run github:skylarsimoncelli/pftui`. Files: new `flake.nix`, `flake.lock`.
- [ ] **Scoop manifest (Windows)** — JSON manifest for Scoop package manager. Downloads Windows binary from GitHub Releases. Files: new `scoop/pftui.json` or submit to scoop extras bucket.
- [ ] **Snap / Flatpak** — Snap and/or Flatpak packaging for broader Linux distribution. Files: `snap/snapcraft.yaml` or `flatpak/com.github.skylarsimoncelli.pftui.yml`.
- [ ] **Docker image** — Minimal container image (`FROM scratch` or Alpine-based) for running pftui in Docker. `docker run -it pftui`. Files: `Dockerfile`, add to release workflow.
- [ ] **Debian/Ubuntu .deb package** — Build `.deb` via `cargo-deb` in release CI. Host a PPA or include `.deb` as GitHub Release asset. `apt install pftui`. Files: add `[package.metadata.deb]` to `Cargo.toml`, update release workflow.
- [ ] **RPM package (Fedora/RHEL/CentOS)** — Build `.rpm` via `cargo-generate-rpm` in release CI. Host a COPR repo or include `.rpm` as GitHub Release asset. `dnf install pftui`. Files: add RPM metadata, update release workflow.

## P0 — Bugs & Layout Fixes (Owner Report)

- [x] **Add easy position modification** — DONE (2026-03-02). Added `A` (add transaction inline form) and `X` (delete position with confirmation) from TUI. See CHANGELOG.

## P0 — CLI & Headless Gaps (Feedback)


## P0 — Setup & Pricing Bugs (Owner Report)

- [ ] **Setup wizard inline suggestions** — The fuzzy matching backend is done (5-tier ranked scoring in search_names). Next step: show ranked matches inline as the user types in the setup wizard, instead of requiring Enter first. Needs raw terminal mode for character-by-character input with live result display below the prompt. Files: `src/commands/setup.rs` (interactive input loop with crossterm raw mode).
- [ ] **Setup wizard: configurable primary fiat currency** — Full mode should let the user choose their primary fiat currency (EUR, GBP, JPY, etc.) instead of hardcoding USD. Default to USD if not specified. Store in `config.toml`. All portfolio values, gains, and display formatting should respect the chosen currency. Files: `src/config.rs`, `src/commands/setup.rs`, `src/tui/widgets/header.rs`, `src/models/position.rs`.

## P0 — Performance Fix (Owner Request)


## P1 — Import/Export (Owner Request)

- [x] **Add `pftui import` command** — DONE (2026-03-02). `pftui import <path> [--mode replace|merge]`. Replace wipes and rebuilds (default), merge adds new entries without deleting. Validates mode match, symbols, quantities, dates, allocation pcts. Full export→import roundtrip tested. 15 new tests. See CHANGELOG.

## P1 — Mock Mode (Owner Request)

- [ ] **Add `pftui demo` command** — Opens pftui with a realistic mock portfolio from a bundled mock DB. `pftui demo` loads a pre-built demo SQLite DB packaged with the repo and launches the TUI against it. The demo mock portfolio should be diverse and realistic:
  - **Commodities:** GLD (Gold), SLV (Silver), COPX (Copper), URA (Uranium), USO (Oil)
  - **Indices/ETFs:** SPY (S&P 500), QQQ (Nasdaq), IWM (Russell 2000)
  - **Crypto:** BTC, ETH, SOL
  - **Forex/Cash:** USD, JPY (via CurrencyShares or similar)
  - **Bonds:** TLT (20Y Treasury), SHY (Short-term)
  - Realistic quantities and cost bases (e.g. 10 SPY @ $420, 0.5 BTC @ $28000, 100 GLD @ $180)
  - Multiple transactions per asset (buys at different dates/prices for realism)
  - Store as `demo.db` in the repo
  - Running `pftui demo` should not overwrite the existing user DB 

## P2 — Web Interface (Owner Request)

- [ ] **Add `pftui web` subcommand** — Spins up a web server serving the portfolio UI in a browser. Subcommands:
  - `pftui web start [--bind <addr>] [--port <port>] [--password <pass>]` — Start server. Default: `127.0.0.1:8080`. Pass `--bind 0.0.0.0` for external access. Optional `--password` enables HTTP basic auth.
  - `pftui web stop` — Stop the running server (write PID file for management)
  - `pftui web status` — Show if running, bound address, port
  - The web UI should share as much logic as possible with the TUI — extract portfolio data computation, sorting, filtering, chart data generation into a shared `core` layer that both TUI and web consume. The web frontend renders the same data, NOT a copy of the TUI rendering code.
  - Tech stack suggestion: `axum` or `warp` for HTTP server, serve a lightweight JS frontend (or HTMX) that calls REST API endpoints backed by the shared core. Keep dependencies minimal.
  - Files: new `src/web/` module (server.rs, routes.rs, static/), refactor shared logic into `src/core/` if not already separated, `src/cli.rs`
  - This is a bigger effort — break into sub-tasks if needed:
    1. [ ] Extract shared core logic from TUI-specific rendering
    2. [ ] Build REST API (positions, transactions, watchlist, chart data, portfolio summary)
    3. [ ] Build minimal web frontend
    4. [ ] Add auth, bind options, PID management

## P0 — CI & Release Pipeline (Owner Request)


## P2 — Remaining Package Managers (Need Owner Action)

- [ ] **Publish to Snapcraft** — snapcraft.yaml is in repo. Needs: 1) Create Snapcraft account at https://snapcraft.io 2) `snapcraft login` and export token 3) Add `SNAPCRAFT_TOKEN` as GitHub repo secret 4) Add snap publish step to release workflow. Files: `snap/snapcraft.yaml`, `.github/workflows/release.yml`.
- [ ] **Publish to AUR** — Needs: 1) Create AUR account at https://aur.archlinux.org 2) Generate SSH key pair 3) Add `AUR_SSH_KEY` as GitHub repo secret 4) Create AUR package `pftui-bin` 5) Add AUR publish step to release workflow. Files: `.github/workflows/release.yml`.
- [ ] **Publish to Scoop** — Needs Windows binary first. Add `x86_64-pc-windows-msvc` target to release workflow build matrix, then submit manifest to scoop-extras bucket or host own bucket. Files: `scoop/pftui.json`, `.github/workflows/release.yml`.
- [ ] **Windows build support** — Add `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` to release build matrix (runs-on: windows-latest). Cross-platform terminal support via crossterm should work. Files: `.github/workflows/release.yml`. Test: verify TUI renders on Windows Terminal.

## Feedback Summary

**Last reviewed:** 2026-03-02

| Tester | Usefulness | Overall | Trend |
|--------|-----------|---------|-------|
| Sentinel Main (Interactive Review) | 25% | 40% | — (first entry) |
| Evening Eventuality Planner | 20% | 38% | — (first entry) |
| Portfolio Analyst | — | — | No data yet |

**Lowest scorer:** Evening Eventuality Planner (20% usefulness, 38% overall) — entirely headless workflow, blocked by lack of CLI price refresh.

**Top 3 priorities based on feedback:**
1. **`pftui refresh` — headless price command** (P0) — Both testers' #1 request. Without it, 4 of 6 positions show N/A outside the TUI. This single feature transforms the tool from a ledger to a live portfolio tracker.
2. ~~**Period-based P&L (`--period` flag)**~~ (P0, DONE) — Both testers need daily/weekly/monthly change, not just total gain from cost basis. Critical for daily briefings and monitoring routines.
3. **Category-grouped summary (`--group-by`)** (P0) — Both testers want "Metals 33%, Crypto 18%, Cash 49%" style output. Currently allocation percentages are meaningless when positions lack prices.

**Notes:**
- All scores are initial baselines — no trends yet. Third tester (Portfolio Analyst) has not submitted feedback.
- The overwhelming signal: **CLI/headless capabilities are the critical gap**. The TUI is well-regarded architecturally, but both testers run headless workflows and get minimal value without CLI price refresh.
- Multi-currency (GBP stored as USD) is a shared pain point but lower priority than price refresh.

## P1 — Mouse Support (Owner Request)

- [ ] **Add mouse click support** — Like htop. All UI elements that look clickable should be clickable. Specifically:
  - Click on a position row to select it
  - Click on tab labels (1-5) to switch views
  - Click on chart to open/focus it
  - Click on sort column headers to sort by that column
  - Click on allocation bars to select that position
  - Click on help overlay items
  - Scroll wheel for scrolling (j/k equivalent)
  - Click on theme/privacy indicators to toggle
  - Right-click context menu (stretch goal)
  - crossterm already supports mouse events via `EnableMouseCapture` — just need to wire up hit-testing for each clickable region
  - Store rendered region rects per frame for hit-testing
  - Files: `src/app.rs` (handle_mouse, hit regions), `src/tui/ui.rs` (track rendered rects), all view files (register clickable regions)

## P2 — Theme Overhaul (Owner Request)

- [ ] **Revamp theme selection** — Current themes are too flat and samey. Every theme should feel bold, dynamic, and visually distinct. Rework existing and add new themes so the full set covers a wide range of aesthetics. Ideas:
  - **Inferno** — deep blacks with fire reds, oranges, and amber accents. Gains glow hot, losses smolder
  - **Neon** — cyberpunk-inspired. Electric pink, cyan, purple on dark. Think synthwave/retrowave
  - **Pastel** — soft, toned-down palette. Muted pinks, blues, greens on a warm gray. Easy on the eyes
  - **Miasma** — warm atmospheric haze. Deep burgundy, dusty orange, olive, muted gold
  - **Hacker** — classic green-on-black terminal aesthetic. Multiple shades of green, minimal color
  - **Dracula** — keep but make more vivid and punchy, lean into the purples
  - **Nord** — keep but add more contrast, feels too washed out currently
  - **Catppuccin** — keep, it's good
  - **Midnight** — keep as default but ensure it's the most polished of all
  - **Solarized** — evaluate if it's distinct enough to keep, otherwise replace
  - **Gruvbox** — evaluate, replace with something more unique if too similar to Miasma
  - Each theme should have strong visual identity — a user should instantly know which theme they're on
  - Test all 28 color slots per theme across all views to ensure nothing looks flat or broken
  - Files: `src/tui/theme.rs`, `src/tui/views/help.rs` (theme preview)

## P0 — Chart Ratio Bugs (Owner Report)




## P0 — Portfolio Chart Broken (Owner Report)



## P1 — Layout Restructure (Owner Request)

- [ ] **Restructure left pane as "PORTFOLIO OVERVIEW"** — The top-left pane should be a comprehensive portfolio overview section with a visible header "PORTFOLIO OVERVIEW". Contents (top to bottom):
  - Portfolio value + day gain/loss (already in header, but also here)
  - Allocation bars
  - Portfolio chart (with timeframe gains: hourly/daily/weekly/monthly/yearly)
  - Key portfolio stats (total positions, top performer, worst performer)
  - Files: `src/tui/ui.rs`, possibly new `src/tui/views/portfolio_overview.rs`
- [ ] **Add "POSITIONS" section header** — Below the portfolio overview, the positions table should have its own visible section header "POSITIONS". Clear visual separation between portfolio overview and positions list. Files: `src/tui/ui.rs`, `src/tui/views/positions.rs`.
- [ ] **Add "ASSET OVERVIEW" header to right pane** — The right pane should have a visible "ASSET OVERVIEW" header. Contains: asset detail header (already exists), asset chart, and any asset-specific data. Files: `src/tui/ui.rs`, `src/tui/widgets/asset_header.rs`.
