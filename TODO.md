# TODO — pftui

> Pick the highest-priority unclaimed `[ ]` item. Mark `[~]` while working, `[x]` when done.
> Each item is scoped to ~1 hour. If it's bigger, split it. Update CHANGELOG.md when done.

## P1 — Animations & Live Feel

- [x] **Add price flash with directional arrows** — When a price updates, show a brief ▲/▼ arrow next to the price that fades after ~1s. Currently we flash the price cell bg, but adding a directional indicator makes the update scannable without reading the number. Use `price_flash_ticks` map (already exists) and extend to store direction (up/down/same). Files: `src/tui/views/positions.rs`, `src/app.rs`. Test: verify flash direction stored on update.
- [~] **Add scrolling ticker tape in header** — Horizontal marquee-style ticker showing top movers from Markets view data: "SPX +1.2% │ BTC -3.4% │ GOLD +0.5%" scrolling left. Renders in the header row using the space after the portfolio value. Uses `app.tick_count` to advance position by 1 char every ~6 ticks (~10 chars/sec). Only active on Positions view to avoid clutter. Files: `src/tui/widgets/header.rs`, `src/app.rs` (market data already available). Test: test ticker text generation and wrap-around.
- [ ] **Add pulsing border on active panel** — Instead of static `border_active` color, pulse the focused panel's border using `pulse_color()` (already exists in theme.rs). Subtle 2-second sine wave between `border_inactive` and `border_active` intensity. Gives the app a "breathing" feel. Only when prices are live (dead/stale = static border). Files: `src/tui/views/positions.rs` (render_table block), `src/tui/widgets/price_chart.rs` (chart block). Test: verify pulse applied only when prices_live.
- [ ] **Add row highlight animation on selection change** — When j/k moves selection, briefly flash the entire new row brighter (lerp from `surface_3` toward `border_accent` then fade back over ~15 ticks). Track `last_selection_change_tick` on App. Files: `src/tui/views/positions.rs` (row_bg calculation), `src/app.rs` (track tick on selection change). Test: test flash decay timing.

## P1 — Header & Status Bar Enhancements

- [x] **Add day gain/loss to header** — Show today's portfolio change alongside total gain: "$45.2k +1.3% ▲$580 today". Compute from sum of (position.quantity × position.day_change_amount). The "today" figure is the most-checked number in any portfolio app. Files: `src/tui/widgets/header.rs` (add today gain span), `src/app.rs` (compute daily portfolio change from position day changes). Test: test daily change computation.
- [x] **Add market status indicator to header** — Show "◉ OPEN" (green) or "◎ CLOSED" (muted) based on current UTC time vs US market hours (9:30-16:00 ET, weekdays). Simple timezone offset check, no external dependency. Renders after the clock in the header. Files: `src/tui/widgets/header.rs`. Test: test market open/closed detection for various UTC times.
- [ ] **Add breadcrumb trail to status bar** — When in chart view, show the navigation path: "Positions › AAPL › 3M Chart › AAPL/SPX". When in detail popup: "Positions › AAPL › Detail". Replaces the generic hint bar text with context-aware breadcrumbs. Files: `src/tui/widgets/status_bar.rs`, `src/app.rs` (expose chart variant label). Test: test breadcrumb string generation for each navigation state.

## P1 — Positions Table Visual Density

- [ ] **Add inline mini-sparkline in price column** — Render a 3-char sparkline (▁▃▇) directly after the price number in the Price column, using the last 3 hours of data (or last 3 history points). Gives instant trend context without looking at the separate Trend column. Files: `src/tui/views/positions.rs` (price cell rendering). Test: verify sparkline renders with various history lengths.
- [ ] **Add color-coded category dot before asset name** — Replace the plain text category label approach with a small colored dot (●) in the asset's category color at the start of the Asset column. More scannable than the current approach of coloring the entire row. The `▎` selection marker already uses this column — put the dot after the marker. Files: `src/tui/views/positions.rs` (asset_line construction). Test: verify dot color matches category.
- [ ] **Add gain/loss magnitude bar in Gain% column** — Render the gain% number with a tiny proportional bar behind it: green fill for gains, red for losses, scaled to ±20%. Like a micro horizontal bar chart in each cell. Uses EIGHTH_BLOCKS for sub-character resolution. Files: `src/tui/views/positions.rs` (gain cell rendering). Test: test bar width calculation for various percentages.

## P2 — Chart Visual Enhancements

- [ ] **Add crosshair cursor on charts** — When chart detail is open, pressing `c` enables a crosshair mode: j/k moves a vertical line across the chart, showing the date and price at that point in a tooltip overlay. Renders as a vertical column of `│` characters in `text_accent` color with a data label. Files: `src/tui/widgets/price_chart.rs` (crosshair rendering), `src/app.rs` (crosshair_mode, crosshair_x fields, c keybinding). Test: test crosshair bounds clamping.
- [ ] **Add chart area fill with gradient** — Instead of just braille dots for the line, fill the area below the line with a fading gradient using BLOCK characters at very low intensity (10-20% alpha via dark versions of chart colors). Creates a "filled area chart" effect common in financial dashboards. Files: `src/tui/widgets/price_chart.rs` (render_braille_chart area fill pass). Test: verify fill doesn't exceed chart line position.
- [ ] **Add Bollinger Bands overlay** — Compute 20-period SMA ± 2 standard deviations. Render as faint dotted braille lines above and below the SMA(20). When price touches a band, highlight the touch point. Shows volatility and overbought/oversold conditions. Files: `src/tui/widgets/price_chart.rs` (compute_bollinger, overlay rendering). Test: test band computation with known data.

## P2 — Layout & Visual Polish

- [ ] **Add Unicode box-drawing panel separators** — Replace the default ratatui `Rounded` border type with custom double-line top (═══) and single-line sides (│). Use `╔═══╗` style for the active panel and `┌───┐` for inactive. Gives a more premium, Bloomberg-like feel. Files: `src/tui/views/positions.rs`, `src/tui/widgets/price_chart.rs`, `src/tui/widgets/allocation_bars.rs`, `src/tui/widgets/portfolio_sparkline.rs`. Test: visual verification only.
- [ ] **Add shadow effect on popups** — When the detail popup or help overlay renders, draw a 1-cell shadow on the right and bottom edges using `surface_0` with slight offset. Creates a floating/elevated look. Files: `src/tui/views/position_detail.rs`, `src/tui/views/help.rs`. Test: verify shadow doesn't exceed terminal bounds.
- [ ] **Add section divider lines between position groups** — When sorted by category, insert thin separator lines (─── Crypto ───) between position groups. Uses `border_subtle` color. Only appears when sort field is Category. Files: `src/tui/views/positions.rs`. Test: test divider insertion logic.
- [ ] **Add ultra-wide layout (160+ columns)** — When terminal is very wide, show a third column: market context panel with major indices and the portfolio sparkline below the positions table, with sidebar remaining as the chart panel. Three-column layout: 45% positions / 25% market context / 30% chart. Files: `src/tui/ui.rs` (new layout branch), new `src/tui/widgets/market_context.rs`. Test: test layout thresholds.

## P2 — Sidebar & Sparkline Enhancements

- [ ] **Add portfolio sparkline period selector** — The sparkline is hardcoded to 90d. Allow cycling with `[`/`]` keys (when sidebar is focused) through 1W, 1M, 3M, 6M, 1Y. Show period label in the sparkline panel title. Reuse the `ChartTimeframe` enum. Files: `src/tui/widgets/portfolio_sparkline.rs` (accept timeframe), `src/app.rs` (sparkline_timeframe field, `[`/`]` keybindings). Test: test timeframe cycling.
- [ ] **Add allocation change indicators** — Show ▲/▼ arrows next to allocation percentages when they've changed since the previous day (based on price movements shifting allocation weights). Helps identify rebalancing needs. Files: `src/tui/widgets/allocation_bars.rs`, `src/app.rs` (store previous day allocations for comparison). Test: test change detection logic.

## P2 — Micro-Interactions & Feedback

- [ ] **Add keystroke echo in status bar** — Briefly flash the last pressed key in the status bar corner: shows "k" for 0.3s when you press k, "gg" for the two-key sequence, "Ctrl+d" etc. Helps users learn keybindings and confirms input was received. Render in `text_muted` with quick fade. Files: `src/tui/widgets/status_bar.rs`, `src/app.rs` (last_key_display, last_key_tick). Test: test key echo text generation.
- [ ] **Add sort indicator animation** — When user changes sort order (s/n/c/% etc.), briefly animate the sort arrow (▲/▼) by flashing it in `text_accent` then fading to normal. Confirms the sort happened. Track `last_sort_change_tick` on App. Files: `src/tui/views/positions.rs` (sort indicator styling), `src/app.rs`. Test: test flash timing.
- [ ] **Add loading skeleton for empty states** — When a view is loading data, show shimmer/skeleton placeholder rows instead of "Waiting for data...". Render 5-6 rows of `░░░░░░` block characters in `text_muted` with a wave animation (phase offset per row). Makes loading feel fast and intentional. Files: `src/tui/views/positions.rs`, `src/tui/views/markets.rs`, `src/tui/views/economy.rs`. Test: verify skeleton row count matches expected.

## P2 — Theme & Color Enhancements

- [ ] **Add theme preview on cycle** — When pressing `t`, show a brief (1.5s) toast notification in the status bar: "◆ Midnight" (with theme name in that theme's accent color). Currently the theme just changes with no feedback about which theme you're on unless you look at the header. Files: `src/tui/widgets/status_bar.rs`, `src/app.rs` (theme_toast_tick). Test: test toast display timing.
- [ ] **Add dynamic header accent based on portfolio performance** — Tint the header border/accent color slightly green when portfolio is up today, slightly red when down. Subtle (5-10% blend) so it doesn't clash with theme, but gives an instant ambient mood indicator. Files: `src/tui/widgets/header.rs`. Test: test color blending for positive/negative days.

## P2 — Data & Infrastructure

- [ ] **Add news feed integration** — Fetch financial news from a free RSS/API source (e.g., Yahoo Finance RSS, Finnhub free tier). Display as a scrollable list: timestamp, headline, source. Per-asset filtering. Files: new `src/news/` module, new `src/tui/views/news.rs`. Research: find best free news API that works without API key.
- [ ] **Add FRED economic data** — FRED API (free with API key) for treasury yields, CPI, unemployment, Fed funds rate. Store in new DB table. Cache aggressively (economic data updates daily at most). Files: new `src/data/fred.rs`, `src/db/economic_cache.rs`.
- [ ] **Add candlestick chart variant** — OHLC candlestick rendering using braille/block characters. Green body for close > open, red for close < open. Wicks as thin lines. Requires OHLC data in HistoryRecord. Files: `src/models/price.rs`, `src/price/yahoo.rs`, `src/tui/widgets/price_chart.rs`.

## P3 — Future

- [ ] **Portfolio analytics** — Sharpe ratio, max drawdown, volatility metrics, benchmark comparison
- [ ] **Dividend tracking** — Track dividend payments, show yield, ex-dates
- [ ] **Correlation matrix** — Visual correlation grid between portfolio positions
- [ ] **Multi-portfolio support** — Multiple named portfolios with switching
- [ ] **Price alerts** — Configurable threshold alerts with terminal notification
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

- [x] **Fix chart timeframe selection** — Many timeframes (1W, 1M, 6M, 1Y, 5Y) don't load data or show empty charts. Only 3M works reliably. Debug: check if history fetch requests the correct number of days, if Yahoo/CoinGecko APIs return data for all periods, and if resampling handles short data correctly. Files: `src/price/mod.rs`, `src/tui/widgets/price_chart.rs`, `src/app.rs`.
- [x] **Fix layout: allocation bars belong in left pane** — The left pane should be the portfolio overview (allocation bars, portfolio sparkline, total portfolio info). The right pane should be the selected asset detail (asset chart, asset info). Currently allocation may be rendering in the wrong pane. Files: `src/tui/ui.rs`, `src/tui/widgets/sidebar.rs`.
- [x] **Fix layout: portfolio chart on left, asset chart on right** — Portfolio-level chart (sparkline/value over time) should be in the left pane. Per-asset price chart should be in the right pane. Establish clear L/R separation: left = portfolio overview, right = selected asset detail. Files: `src/tui/ui.rs`.
- [x] **Make asset detail info permanent in right pane header** — The asset overview popup that appears when cycling through charts should be permanently displayed at the top of the right pane (not a popup). Show: symbol, name, price, gain/loss, quantity, allocation% — always visible above the asset chart. Files: `src/tui/ui.rs`, `src/tui/widgets/price_chart.rs`, possibly new `src/tui/widgets/asset_header.rs`.
- [ ] **Add easy position modification** — There's no easy way to modify existing positions from the TUI. Add keybinding (e.g., `a` to add transaction, `d` to delete transaction for selected asset) that opens an inline form or spawns the CLI flow. Files: `src/app.rs`, possibly new `src/tui/views/edit_position.rs`.

## P0 — Setup & Pricing Bugs (Owner Report)

- [ ] **Setup wizard fuzzy finder** — The asset symbol entry in the setup wizard should have fuzzy autocomplete. As the user types, show matching symbols/names from the asset_names map (and any custom symbols). Use a ranked fuzzy match (not just prefix). Show results inline below the input. Files: `src/commands/setup.rs`, `src/models/asset_names.rs` (search_names already exists — wire it into interactive input).
- [ ] **Setup wizard: configurable primary fiat currency** — Full mode should let the user choose their primary fiat currency (EUR, GBP, JPY, etc.) instead of hardcoding USD. Default to USD if not specified. Store in `config.toml`. All portfolio values, gains, and display formatting should respect the chosen currency. Files: `src/config.rs`, `src/commands/setup.rs`, `src/tui/widgets/header.rs`, `src/models/position.rs`.
- [x] **Fix BTC price fetching** — BTC price fails to load for at least one user. Debug: check CoinGecko→Yahoo fallback chain, verify `BTC-USD` symbol resolves, check rate limiting / API errors. Add better error logging to identify where the fetch fails. Files: `src/price/coingecko.rs`, `src/price/yahoo.rs`, `src/price/mod.rs`.

## P0 — Performance Fix (Owner Request)

- [x] **Switch to on-demand chart history fetching** — Currently fetches 5Y of daily data for every asset upfront (wasteful, slow startup, rate limiting risk). Change to:
  - Default initial fetch: 3M (90 days) — covers the most common view
  - On timeframe switch: if requested range exceeds cached data, fetch the delta and merge (merge_history_into already exists)
  - Cache for session: once 1Y data is fetched for an asset, don't re-fetch if user switches back to 3M then 1Y
  - This reduces startup API calls, avoids CoinGecko rate limiting, and saves bandwidth
  - Files: `src/price/mod.rs` (request_all_history, request_history_for_symbol), `src/app.rs` (trigger fetch on timeframe change if needed)

## P1 — Import/Export (Owner Request)

- [ ] **Add `pftui export` command** — Dump the full database (positions, transactions, watchlist, config) to a JSON or CSV file. Default: JSON (preserves types). `pftui export [--format json|csv] [--output <path>]`. If no output path, print to stdout. JSON should be a complete snapshot: `{ "positions": [...], "transactions": [...], "watchlist": [...], "config": {...} }`. Files: new `src/commands/export.rs`, `src/cli.rs`, `src/db/mod.rs` (query helpers).
- [ ] **Add `pftui import` command** — Import data from a JSON or CSV file, overwriting the current DB. `pftui import <path> [--format json|csv] [--merge|--replace]`. `--replace` wipes and rebuilds (default), `--merge` adds new entries without deleting existing. Validate schema before writing. Prompt for confirmation on `--replace`. Files: new `src/commands/import.rs`, `src/cli.rs`.

## P1 — Mock Mode (Owner Request)

- [ ] **Add `pftui mock` command** — Opens pftui with a realistic mock portfolio from a bundled mock DB. `pftui mock` copies a pre-built SQLite DB to a temp location and launches the TUI against it. The mock portfolio should be diverse and realistic:
  - **Commodities:** GLD (Gold), SLV (Silver), COPX (Copper), URA (Uranium), USO (Oil)
  - **Indices/ETFs:** SPY (S&P 500), QQQ (Nasdaq), IWM (Russell 2000)
  - **Crypto:** BTC, ETH, SOL
  - **Forex/Cash:** USD, JPY (via CurrencyShares or similar)
  - **Bonds:** TLT (20Y Treasury), SHY (Short-term)
  - Realistic quantities and cost bases (e.g. 10 SPY @ $420, 0.5 BTC @ $28000, 100 GLD @ $180)
  - Multiple transactions per asset (buys at different dates/prices for realism)
  - Store as `mock/portfolio.db` in the repo (or `mock/portfolio.json` and build DB on first run)
  - Files: new `src/commands/mock.rs`, new `mock/portfolio.json`, `src/cli.rs`

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
