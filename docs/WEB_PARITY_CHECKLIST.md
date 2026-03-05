# Web Parity Checklist

Source: user-provided parity checklist. This file is the implementation tracker and is updated as items are completed.

## Target
- [ ] Full TUI parity in web: same information architecture, same mental model, web-native interactions, and shared Rust domain logic.

## Implementation Plan
- [x] 1. Lock the shared contract first.
- [x] 2. Define a single Rust web/view_model layer that exports normalized payloads for all tabs (positions, watchlist, markets, economy, news, journal, alerts, transactions, portfolio_performance).
- [x] 3. Move duplicated label/color/tab/theme constants into shared Rust modules and expose via `/api/ui-config`.
- [x] 4. Add versioned API schema docs in-repo so frontend and backend evolve together without drift.

## Core Parity Tabs
- [x] 5. Finish core parity tabs.
- [x] 6. Complete Transactions tab behavior and table fields to match TUI sort/filter semantics.
- [x] 7. Complete Markets tab with selected row state, top movers, and right-pane chart behavior.
- [x] 8. Complete Economy tab sections (yields, commodities, currencies, macro cards) with TUI ordering and visual priority.
- [x] 9. Complete Watchlist tab with targets/proximity and click-through chart behavior.
- [x] 10. Implement full News tab from existing news data sources.
- [x] 11. Implement full Journal tab list/search/detail workflow.

## Navigation and State
- [x] 12. Replicate TUI navigation model in web-native form.
- [x] 13. Keep top tabs equivalent to 1-7 views.
- [x] 14. Add keyboard shortcuts matching TUI intent (`/` search, `j/k` row movement, `enter` open detail, `t` cycle theme).
- [x] 15. Persist user context: active tab, selected symbol, timeframe, theme, filters, privacy mode.
- [x] 16. Add compact/mobile layout preserving the same hierarchy (left data pane, right context pane pattern).

## Portfolio Chart Parity
- [x] 17. Portfolio chart parity.
- [x] 18. Add true portfolio curve generation from snapshots.
- [x] 19. Add fallback when snapshots are missing: aggregate current holdings -> stitch per-symbol historical closes -> reconstruct portfolio history.
- [x] 20. Match TUI timeframes exactly (1W, 1M, 3M, 6M, 1Y, 5Y) and keep selection persistent.
- [x] 21. Add TUI-like overlays: benchmark compare, drawdown, gain/loss coloring.

## Themes and Visual Design
- [ ] 22. Theme parity and design tokens.
- [x] 23. Keep all 11 TUI theme names and derive web CSS variables from Rust theme structs only.
- [x] 24. Add theme cycle control and optional theme picker modal.
- [ ] 25. Ensure contrast/a11y checks per theme.
- [ ] 26. Add visual polish parity: panel hierarchy, border emphasis, selected-row treatment, muted/active text states.

## Detail Panels and Overlays
- [x] 27. Detail panels and overlays parity.
- [x] 28. Asset detail drawer/modal matching TUI details (core stats, technicals, ranges, context).
- [x] 29. Alerts overlay and indicator badges.
- [x] 30. Search overlay behavior equivalent to TUI search/filter patterns.
- [x] 31. Add loading/skeleton/error states for each panel.

## Data Refresh Model
- [x] 32. Data freshness and refresh model.
- [x] 33. Match TUI refresh cadence and stale-data indicators.
- [x] 34. Add optional SSE/WebSocket push updates for quote/market changes.
- [x] 35. Keep fallback polling.
- [x] 36. Surface last refresh timestamp and source health in UI.

## Auth and Session Hardening
- [x] 37. Auth/session hardening.
- [x] 38. Replace injected static token pattern with secure session auth for web mode.
- [x] 39. Keep `--no-auth` local dev shortcut.
- [x] 40. Add CSRF/session expiry handling if browser auth is enabled.
- [x] 41. Add explicit unauthenticated UI state.

## Testing and Release
- [ ] 42. Test parity and quality gates.
- [ ] 43. Add backend API tests per endpoint payload contract.
- [ ] 44. Add frontend integration tests for tab flows and chart loads.
- [ ] 45. Add parity checklist against TUI for every release.
- [ ] 46. Add visual regression snapshots across desktop/mobile and all themes.

## Rollout Sequence
- [ ] 47. Rollout sequence.
- [x] 48. Milestone A: complete tab data parity and shared view-model API.
- [x] 49. Milestone B: complete UX/navigation parity and persistence.
- [x] 50. Milestone C: complete chart parity fallback and detail overlays.
- [ ] 51. Milestone D: harden auth, tests, and publish as stable `pftui web`.

## Progress Notes
- 2026-03-05: Phase A baseline fix landed for new `Config.home_tab` field by updating explicit `Config { ... }` initializers in tests (`src/app.rs`, `src/commands/export.rs`).
- 2026-03-05: Phase B landed session auth (`/auth/login`, `/auth/logout`, `/auth/session`, `/auth/csrf`), middleware CSRF enforcement for mutating `/api/*`, and unauthenticated/expired-session UI flow in web frontend.
- 2026-03-05: Phase C landed overlay stack + keyboard parity for search/alerts/asset details (single active overlay, Esc-close priority, focus return), plus alert badges in header/tab.
- 2026-03-05: Phase D landed `/api/stream` SSE events (`quote_update`, `panel_invalidate`, `health`, `heartbeat`) with frontend reconnect/backoff and automatic polling fallback status.
