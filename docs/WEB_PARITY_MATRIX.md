# Web <-> TUI Parity Matrix

Last updated: 2026-03-05
Owner: web parity program
Source of truth: TUI behavior and domain contracts (`src/app.rs`, `src/tui/views/*`)

## Status Legend
- `implemented`: behavior exists in web and matches TUI intent
- `partial`: behavior exists but misses fields/workflow depth/parity
- `missing`: behavior not available in web
- Severity: `P0` blocks core battlestation workflow, `P1` major usability gap, `P2` polish

## Contract Freeze (Phase 0)
| Domain | Frozen Contract |
|---|---|
| Tabs | `Positions, Transactions, Markets, Economy, Watchlist, Alerts, News, Journal` (Transactions hidden in percentage mode) |
| Timeframes | `1W, 1M, 3M, 6M, 1Y, 5Y` |
| Core keys | `/`, `j/k`, `Enter`, `Esc`, `t` |
| Home tab persistence | `positions` or `watchlist` only |
| Alert status vocabulary | `armed`, `triggered`, `acknowledged` |
| Tx sort keys | `date`, `symbol`, `type`, `qty`, `price`, `fee` |

## Parity Rows
| ID | Area | TUI Baseline | Web Current | Status | Severity | Owner Phase | Acceptance Criteria |
|---|---|---|---|---|---|---|---|
| NAV-01 | Tab model | 1-7 view parity | Present | implemented | P1 | 0 | Same tab names and availability rules |
| NAV-02 | Keyboard parity | `/`, `j/k`, `Enter`, `Esc`, `t`, view switches | Keyboard intents now hardened across tabs: numeric view switching, slash overlay, j/k movement, Enter activation, Esc close order, and theme cycling | implemented | P1 | 10 | All shared key intents match behaviorally |
| NAV-03 | Overlay stack | Search + asset detail + alerts layering | Present | implemented | P2 | 10 | Deterministic Esc close order + focus return |
| POS-01 | Right pane context | Asset-specific overview in positions | Positions pane now uses asset-only overview context | implemented | P0 | 4 | Right pane always names selected asset and shows asset overview only |
| POS-02 | Market overview placement | Markets domain lives in Markets tab | Market overview consolidated to Markets tab only | implemented | P1 | 4 | Market overview removed from positions and consolidated in markets |
| POS-03 | Asset naming | Selected asset should be explicit | Selected symbol reflected in positions right-pane header | implemented | P0 | 4 | Selected symbol/name visible in right pane header |
| SRCH-01 | Global search | Search all assets + contextual result state | API-backed global symbol search added | implemented | P0 | 3 | Global universe search (not only already-loaded rows) |
| SRCH-02 | Search result deep dive | Full asset popup with chart+data | Drawer now loads enriched asset payload (history, changes, range, volume, position) | implemented | P0 | 3 | Popup includes chart + key market/technical/position context |
| SRCH-03 | Search -> watchlist | Quick add/watch from search detail | Star/unstar action in asset drawer from search flow | implemented | P0 | 3 | Star/unstar available from search result overlay |
| WCH-01 | Watchlist quoting | TUI quote symbol normalization (`BTC` -> `BTC-USD`) | Previously partial | implemented | P1 | 1 | Web uses same normalization contract |
| WCH-02 | Watchlist proximity | Target + distance + hit semantics | Present | implemented | P1 | 1 | Same direction semantics and target-hit calculation |
| WCH-03 | Watchlist mutation | Add/remove/set target | Add/remove now implemented via search/detail; target set still pending | partial | P0 | 3/5 | Web can star/unstar and set target path |
| ALT-01 | Alert visibility | Alerts panel + statuses | Alerts tab/overlay now show status with lifecycle actions | implemented | P1 | 5 | List shows status and context parity |
| ALT-02 | Alert lifecycle | add/remove/ack/rearm | Create/remove/ack/rearm endpoints + UI wired | implemented | P0 | 5 | Full CRUD + state transitions via web |
| JRN-01 | Journal list/search | Filter/search rows + detail | Present with search + editable detail | partial | P1 | 6 | Search/filter parity with backend query capabilities |
| JRN-02 | Journal mutation | add/update/remove | Create/update/delete endpoints + UI controls wired | implemented | P0 | 6 | Full create/edit/delete flows |
| TX-01 | Transaction table semantics | Sort/filter parity by known keys | Present partial | partial | P1 | 1/7 | Uses normalized sort/filter contract |
| TX-02 | Transaction mutation | add/remove/edit workflows | Create/edit/delete endpoints + Transactions tab form/actions wired | implemented | P0 | 7 | Full create/edit/delete flows |
| MKT-01 | Markets universe | TUI markets universe + ordering | Previously hardcoded subset | implemented | P0 | 1 | Web derives market overview list from shared contract |
| MKT-02 | Markets movers | meaningful movers (change-based) | Markets tab now uses `/api/macro` movers, change-based sorting, and breadth stats (up/down/flat + strongest/weakest) | implemented | P1 | 8 | Top movers from actual change values |
| ECO-01 | Economy sections | Yields/Currency/Commodities/Volatility structure | Previously narrow grouping | implemented | P0 | 1 | Sections derive from shared economy contract |
| ECO-02 | Economy depth | BLS/sentiment/calendar/predictions richness | Economy tab now renders cache-backed BLS pulse, sentiment snapshot, upcoming calendar events, and prediction markets alongside grouped sections | implemented | P0 | 8 | Data-dense economy panels with key macro cards |
| NEWS-01 | News timeline | Chronological, filterable feed | News tab now renders grouped timeline-by-date with source/category/hour/search filters over `/api/news` and preserves selected-story detail panel | implemented | P0 | 9 | Timeline-first UX + reliable feed ingestion |
| DATA-01 | Refresh runtime | TUI refresh + cached population | Web now runs background refresh and RSS loops | implemented | P0 | 2 | `pftui web` self-populates core caches |
| DATA-02 | Freshness indicators | Live/stale/source status | Present | implemented | P2 | 2 | Metadata reflects actual pipeline status |
| AUTH-01 | Session/CSRF | secure session auth | Present | implemented | P1 | done | No regression |
| TEST-01 | Contract tests | endpoint contract coverage | Partial | partial | P1 | 11 | New mutating endpoints + contract guards covered |
| TEST-02 | E2E parity flows | key workflows tested | Partial | partial | P1 | 11 | CRUD/search/nav critical paths covered in Playwright |

## Cross-Session Update Rule
Any session changing behavior must update:
1. This matrix row(s) (`status`, `owner phase`, `acceptance` if changed).
2. `docs/WEB_REBUILD_CHECKLIST.md` task state.
3. `docs/WEB_API_SCHEMA_v1.md` endpoint status when API is affected.
