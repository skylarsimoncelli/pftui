# Mobile Webapp Design — pftui

**Author:** Product Design & Frontend Architecture  
**Date:** 2026-03-04  
**Status:** Proposal  

---

## 1. Product Vision

### Mobile Use Case

pftui's mobile webapp is **NOT** trying to be a full-featured Bloomberg Terminal in your pocket. That's a trap. The mobile use case is fundamentally different:

**Primary:** **Quick portfolio pulse check** — "How am I doing today?" in under 5 seconds. Standing in line at the grocery store. Walking between meetings. Lying in bed at 6am before work. The user wants to know: total value, daily P&L, what moved, any alerts. That's it.

**Secondary:** **Tactical research** — User is away from their desk and needs to look up a price, check a chart, or verify a position before making a decision. "Should I sell this position now or wait?" They need the data, not the full analytical environment.

**Tertiary (post-MVP):** **Lightweight data entry** — Add a quick transaction while traveling. Mark a journal entry with a voice memo. Set a watchlist target. NOT building a full transaction editor on mobile — that's desktop work.

**What mobile is NOT:**
- Full portfolio rebalancing UI (too complex)
- Deep multi-asset correlation analysis (wrong form factor)
- Regime signal tuning (power user, desktop-only feature)
- Transaction import/export flows (file management on mobile is painful)

### Relationship to TUI

**Companion, not replacement.** The TUI is the power tool for serious analysis. The webapp is the quick-check dashboard.

Think of it like:
- **TUI** = Bloomberg Terminal at your desk
- **Mobile webapp** = Bloomberg app on your phone

The user maintains ONE portfolio database (SQLite on their machine or cloud-synced). Both interfaces read/write the same data. The mobile webapp is a view, not a fork.

**Different audience consideration:** Some users will ONLY use the webapp. They don't want to learn vim keys. They don't care about terminal aesthetics. They just want a clean portfolio tracker that works on their phone. That's fine. The webapp should be fully functional for read-only portfolio tracking without ever touching the TUI.

### Competitive Landscape

| App | Strengths | Weaknesses | pftui Differentiation |
|-----|-----------|------------|----------------------|
| **Yahoo Finance** | Free, comprehensive news, great charts | Cluttered UI, ad-heavy, portfolio tracking is weak | Clean UI, zero ads, privacy-first, offline-capable |
| **Robinhood** | Best mobile trading UX | Locked to their brokerage, no multi-asset support | Multi-exchange, crypto + stocks + commodities, no brokerage lock-in |
| **Delta** | Beautiful crypto tracking | Crypto-only, no traditional assets, abandoned by devs | Unified portfolio (stocks + crypto + gold in one view) |
| **CoinGecko** | Best crypto data | Crypto-only, cluttered with DeFi noise | Serious investor focus (no meme coins), clean UI |
| **Personal Capital** | Great wealth management | Requires account linking (privacy nightmare), US-only banks | Local-first, no account linking, full manual control |
| **Bloomberg** | Professional-grade data | $300/month, desktop-first, overwhelming for retail | Free, mobile-optimized, focused feature set |

**What makes pftui mobile unique:**

1. **Local-first with optional sync** — Your portfolio lives in SQLite on your device or self-hosted server. No third-party ever sees your holdings. Zero privacy compromise.

2. **Offline-capable PWA** — Service worker caching means you can check your portfolio on a plane with yesterday's prices. Graceful degradation when network is unavailable.

3. **Unified multi-asset** — Stocks, crypto, forex, commodities, precious metals in ONE portfolio view. No other free app does this cleanly.

4. **Macro-aware** — Not just "here's your portfolio value" — shows VIX, DXY, 10Y yield, oil, gold context alongside your positions. Institutional-quality macro dashboard in a consumer app.

5. **Zero ads, zero tracking** — No Google Analytics, no ad networks, no "share your portfolio for free premium." Just a clean tool.

6. **Developer-friendly** — CLI-first design means the same backend powers the webapp. No duplicate logic. The API is the product.

---

## 2. Architecture Options

### Option A: PWA (Progressive Web App) ★ RECOMMENDED

**What it is:** The axum server serves a responsive single-page app with a service worker. Users "Add to Home Screen" and it behaves like a native app — full-screen, offline-capable, fast startup.

**Pros:**
- **Best offline support** — Service worker caches the app shell + static assets. Stale-while-revalidate for price data.
- **Installation without app store** — No Apple/Google approval process. Just visit the URL and "Add to Home Screen."
- **Push notifications possible** — Web Push API works on Android (iOS is limited but improving).
- **Single codebase for mobile + desktop** — Responsive breakpoints handle both.
- **Future-proof** — PWAs keep getting better (iOS finally caught up in 2024).

**Cons:**
- **iOS limitations** — Service workers work on iOS but with quirks (storage limits, can be purged by OS). Safari is perpetually 2 years behind Chrome.
- **No app store presence** — Discoverability is harder. Users have to find the URL. (Can submit PWA to Play Store with Trusted Web Activity, but not iOS App Store.)
- **Biometric auth is clunky** — WebAuthn exists but not as smooth as native Face ID integration.

**Effort estimate:** 40-60 hours for MVP (service worker + responsive UI + offline handling)  
**Maintenance burden:** Low — web standards move slowly, one codebase  
**Offline capability:** ★★★★★ Excellent with service worker  

---

### Option B: Responsive Dashboard (No PWA)

**What it is:** Just make the existing `pftui web` dashboard responsive. No service worker, no "Add to Home Screen," no offline mode. Users bookmark the URL and use it in their mobile browser.

**Pros:**
- **Fastest path to mobile** — CSS media queries + mobile-friendly layout. Could ship in 20 hours.
- **No new concepts** — Just a responsive website. Standard web dev.
- **Works everywhere** — Any browser, any device.

**Cons:**
- **No offline support** — Requires network. Fails on planes, tunnels, rural areas.
- **No home screen icon** — Stays in the browser. Feels less "app-like."
- **No push notifications** — Can't alert users when price targets are hit.
- **Missed opportunity** — In 2026, users expect offline-capable financial apps. This feels dated.

**Effort estimate:** 20-30 hours for MVP  
**Maintenance burden:** Low  
**Offline capability:** ★☆☆☆☆ None  

---

### Option C: Separate Mobile-First Frontend (`/mobile` route)

**What it is:** Dedicated mobile UI served from the same axum backend but at a different route. Desktop users hit `/`, mobile users hit `/mobile`. Optimized touch interactions, bottom nav, swipe gestures.

**Pros:**
- **Mobile-optimized UX** — No compromises for desktop layout. Pure mobile-first design.
- **Better touch interactions** — Bottom sheet modals, swipe-to-refresh, long-press menus feel native.
- **Smaller JS bundle** — Don't ship desktop features to mobile users.

**Cons:**
- **Duplicate UI logic** — Two frontends to maintain. Bug fixes need to be applied twice.
- **Routing complexity** — Need user-agent detection or manual route selection.
- **More code** — ~1.5x the frontend code vs a single responsive app.
- **Still a web app** — Doesn't solve offline, notifications, or app store presence.

**Effort estimate:** 50-70 hours for MVP (build a second UI from scratch)  
**Maintenance burden:** High — two UIs  
**Offline capability:** ★☆☆☆☆ None (unless you also add service worker, then same as Option A)  

---

### Option D: Capacitor/Tauri Mobile (Native Wrapper)

**What it is:** Wrap the webapp in a native container (Capacitor or Tauri) for distribution via iOS App Store and Google Play Store.

**Pros:**
- **App store presence** — Users can discover and install via stores.
- **Full native API access** — Biometric auth, native notifications, file system, background sync.
- **Best "app" feel** — Indistinguishable from a native Swift/Kotlin app.
- **Offline is guaranteed** — App bundle includes all assets.

**Cons:**
- **Requires app store approval** — Apple's review process is slow and arbitrary. Could take weeks per release.
- **Maintenance explosion** — Now managing iOS build tooling, Android Gradle, native plugins, two stores, certificate renewals, platform-specific bugs.
- **Duplication with PWA** — If we're building a PWA anyway, this is redundant. Users who want "app-like" can just install the PWA.
- **Code signing costs** — $99/year for Apple Developer account. Google Play is one-time $25 but still a barrier.
- **Slower iteration** — Every release goes through app review. Hot-fixing a bug takes days, not minutes.

**Effort estimate:** 80-120 hours for MVP (native integrations, store submissions, testing on real devices)  
**Maintenance burden:** Very High — native tooling is painful  
**Offline capability:** ★★★★★ Excellent (bundled app)  

---

### Recommendation: **Option A (PWA)** with a future path to Option D if demand is high

**Why PWA wins:**

1. **Best ROI** — One codebase, works on all platforms, offline-capable, installable, future-proof.
2. **Faster iteration** — Deploy fixes instantly. No app store review delay.
3. **Privacy story** — Self-hosted PWA = zero third-party data sharing. Native app stores require privacy policies and compliance.
4. **Low barrier to entry** — Users can try the app instantly via URL. No "install this 50MB app" friction.

**When to consider Option D:**
- User feedback says "I need this in the App Store" (gauge demand first)
- Push notifications become critical (web push works on Android, but native is better)
- Biometric auth becomes a hard requirement (WebAuthn works but is clunky)
- We want search/discovery via app stores

**Implementation path:**
1. Ship PWA first (Option A)
2. Gather feedback for 3-6 months
3. If users demand App Store presence, wrap the PWA in Capacitor (low effort since the app already exists)

---

## 3. Mobile UX Design

### Navigation: Bottom Tab Bar (iOS-style)

**Why bottom tabs win on mobile:**
- Thumb-reachable on large phones (vs top nav = stretch)
- Industry standard (every major finance app uses bottom tabs)
- Clear affordance (users know it's tappable)
- Persistent context (always visible)

**Tab layout (5 tabs max — any more is cluttered):**

```
┌─────────────────────────────────────┐
│  pftui                    🔔 ⚙️      │ ← Header (app name, alerts, settings)
│                                      │
│                                      │
│  [Content area — see wireframes]     │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
├──────────────────────────────────────┤
│  📊    📈    🌍    👁️    📝          │ ← Bottom nav (always visible)
│ Positions Markets Macro Watch Journal│
└──────────────────────────────────────┘
```

**Tab definitions:**

1. **Positions** 📊 — Your holdings. Default home screen. Total value, daily P&L, allocation donut, position cards.
2. **Markets** 📈 — Market pulse. SPX, NDX, BTC, Gold, VIX, 10Y. Quick charts. Top movers.
3. **Macro** 🌍 — Economy dashboard. Yields, DXY, commodities, FRED indicators.
4. **Watchlist** 👁️ — Assets you're tracking. Price targets, distance from target.
5. **Journal** 📝 — Decision log. Recent entries, search, quick-add.

**Transactions tab is GONE on mobile.** Too detailed. If users need to review transactions, they use the desktop TUI or run `pftui list-tx` CLI. Mobile is not for auditing your full transaction history.

**Settings moved to header icon (⚙️)** — Gear icon in top-right opens a sheet with: theme, privacy mode, base currency, sync settings, about/version.

---

### Portfolio View (Positions Tab)

**Layout: Card-based, scannable, finger-friendly**

The TUI shows a dense table because terminal users have big screens and precision pointers. Mobile users have thumbs and small screens. Cards are the answer.

**Information hierarchy:**

1. **Hero metrics (top)** — Total portfolio value, daily P&L ($ and %), visual gain indicator (big green up arrow or red down arrow)
2. **Allocation donut** — Tap to toggle between category donut (Cash/Crypto/Equity/Commodity) and individual position donut. Swipe to switch between allocation % and absolute $.
3. **Position cards (scrollable list)** — One card per position

**Position card anatomy:**

```
┌─────────────────────────────────────┐
│ 🪙 Bitcoin (BTC)              +4.1% │ ← Symbol, name, 1D%
│ $73,705                              │ ← Current price
│ ▂▃▅▆▇▅▃▂▁▃▅ 30D                     │ ← Sparkline
│ 0.25 BTC · $18,426 · 20.0%          │ ← Qty · Value · Allocation
│ Gain: +$3,220 (+21.2%) ✓            │ ← Total gain + checkmark if positive
└─────────────────────────────────────┘
```

**Why cards over table rows:**
- More breathing room (easier to tap the right item)
- Can show sparkline (impossible in cramped table row)
- Richer data per item without feeling dense
- Familiar pattern (every modern mobile app uses cards)

**Interactions:**
- **Tap card** → Position detail sheet (see below)
- **Long press** → Quick actions menu (Add transaction, View chart, Remove position)
- **Pull down** → Refresh prices
- **Swipe left on card** → Delete position (with confirmation)

---

### Position Detail Sheet

When you tap a position card, a bottom sheet slides up (iOS-style) with full details:

```
┌─────────────────────────────────────┐
│         Bitcoin (BTC)          [×]   │
│                                      │
│        $73,705                       │
│        +$3,012 (+4.1%)              │
│                                      │
│  ┌─────────────────────────────┐   │
│  │                              │   │
│  │    [Full 30D chart]          │   │
│  │                              │   │
│  └─────────────────────────────┘   │
│                                      │
│  Position                            │
│  ─────────                           │
│  Quantity:     0.25 BTC              │
│  Avg Cost:     $60,804               │
│  Total Cost:   $15,201               │
│  Current:      $18,426               │
│  Gain/Loss:    +$3,220 (+21.2%)     │
│  Allocation:   20.0%                 │
│                                      │
│  [View Full Chart] [Add Transaction] │
└─────────────────────────────────────┘
```

**Tap "View Full Chart"** → Opens full-screen chart view (see Charts section)

**Tap "Add Transaction"** → Inline form (Buy/Sell picker, quantity input, price input, date picker, submit)

---

### Charts: Full-Screen on Tap, Embedded TradingView

**The problem:** The TUI uses beautiful braille charts. Those don't work on mobile touchscreens. You can't tap a braille chart to see exact values. You can't pinch-zoom.

**The solution:** TradingView's Advanced Chart Widget.

**Why TradingView:**
- Free for non-commercial use (or very cheap commercial license)
- Handles touch interactions perfectly (pinch zoom, pan, tap for crosshair)
- Professional-quality rendering (same charts used by Binance, eToro, etc.)
- Supports all asset types (stocks, crypto, forex, commodities)
- Technical indicators built-in (SMA, RSI, MACD, Bollinger Bands)
- Lightweight embed (just an iframe or JS widget)

**Fallback:** For assets not on TradingView (e.g., `U.UN` Toronto stock), fall back to a server-rendered SVG chart from pftui's price history data. Simple line chart, no interactions. Clearly labeled as "limited data."

**Chart interaction flow:**

1. User taps position card
2. Detail sheet opens with **30-day mini chart** (static SVG or canvas, non-interactive)
3. User taps "View Full Chart" button
4. **Full-screen chart overlay** slides up (covers entire screen)
5. TradingView widget loads with symbol's data
6. User can: pan, zoom, add indicators, switch timeframes (1D/1W/1M/3M/6M/1Y/5Y)
7. Tap `X` in corner to close and return to detail sheet

**Chart caching:** TradingView widget caches chart images locally. Once loaded, subsequent views are instant.

---

### Watchlist View

**Layout: List with price targets**

Watchlist is simpler than positions (no quantity, cost basis, or gain data). Just a list of assets you're monitoring.

**Watchlist item:**

```
┌─────────────────────────────────────┐
│ TSLA                           -2.3% │
│ Tesla Inc.                           │
│ $285.40                              │
│ Target: $300 ▲ (+5.1% away)         │
│ ▂▃▅▆▇▅▃▂▁▃▅ 30D                     │
└─────────────────────────────────────┘
```

**Interactions:**
- Tap item → Asset detail sheet (same as position detail but without cost basis section)
- Swipe left → Remove from watchlist
- `+` button (top-right) → Search overlay to add new symbol

**Price target logic:**
- If user set a target, show it with distance and direction (▲ if current < target, ▼ if current > target)
- If target is within 3%, highlight the card (yellow border or pulse animation)
- If target is hit, show in Alerts (🔔 icon in header gets a badge)

---

### Macro Dashboard (Markets + Economy Tabs Combined)

On desktop, Markets and Economy are separate tabs. On mobile, they merge into **"Macro"** tab.

**Why merge:**
- Limited tab space (5 tabs max)
- Markets and Economy serve the same purpose: context for your portfolio decisions
- Users don't care about the distinction — they just want "what's happening in the world?"

**Macro tab layout:**

```
┌─────────────────────────────────────┐
│  Macro                               │
│                                      │
│  📊 Equities                         │
│  SPX      5,234    +0.3%    ▃▅▇     │
│  NDX     18,401    +0.5%    ▃▆▇     │
│  DJI     41,203    +0.1%    ▂▃▄     │
│                                      │
│  💰 Commodities                      │
│  Gold     2,165    -0.8%    ▇▅▃     │
│  Oil       78.40   +1.2%    ▂▄▆     │
│  Silver    25.30   -1.5%    ▇▄▂     │
│                                      │
│  📈 Rates & Vol                      │
│  10Y       4.23%   +2bps    ▂▃▅     │
│  VIX      14.30    -5.0%    ▅▃▂     │
│  DXY     104.20    +0.4%    ▂▃▄     │
│                                      │
│  🌍 Currencies                       │
│  EUR/USD   1.0820  -0.2%    ▄▃▂     │
│  USD/JPY 150.30    +0.1%    ▂▃▃     │
└─────────────────────────────────────┘
```

**Grouped by category, collapsible sections.** Tap section header to collapse/expand.

**Tap any item** → Full-screen chart (TradingView widget)

---

### Alerts

**Where:** Red badge on 🔔 icon in header when alerts are triggered.

**Tap 🔔** → Alerts list sheet:

```
┌─────────────────────────────────────┐
│  Alerts                        [×]   │
│                                      │
│  🔴 Bitcoin above $75,000            │
│      Triggered 2h ago               │
│      Current: $75,204               │
│                                      │
│  🟡 Gold allocation above 30%        │
│      Triggered today                │
│      Current: 31.2%                 │
│                                      │
│  🟢 VIX below 15                     │
│      Triggered 6h ago               │
│      Current: 14.3                  │
│                                      │
│  [Manage Alerts]                     │
└─────────────────────────────────────┘
```

**Types of alerts:**
1. **Price alerts** — "Notify when BTC > $75k"
2. **Allocation drift** — "Notify when Gold allocation > 30%"
3. **Indicator thresholds** — "Notify when VIX < 15"

**Push notifications (PWA only, Android-first):**
- User enables notifications permission on first launch
- When alert triggers, push notification via Web Push API
- Clicking notification opens the app to the relevant position/chart
- iOS support is limited — Safari doesn't support Web Push reliably yet (as of 2026). Android works perfectly.

**Fallback for iOS:** In-app badge only. When user opens the app, they see the alert badge and can tap to view.

---

### Search Overlay

**Trigger:** Tap search icon in header OR tap `+` in Watchlist tab

**Overlay covers the screen** (like Spotlight on iOS):

```
┌─────────────────────────────────────┐
│  [Search symbol or name...]    [×]   │
│                                      │
│  Recent:                             │
│  TSLA · AAPL · BTC                   │
│                                      │
│  ─────────────────────────────────── │
│                                      │
│  (User types "goo")                  │
│                                      │
│  Results:                            │
│  GOOGL   Alphabet Inc.        Equity │
│  GC=F    Gold Futures      Commodity │
│  GOOG    Alphabet (Class C)   Equity │
│                                      │
│  (Tap result)                        │
└─────────────────────────────────────┘
```

**After selecting a result:**
- If it's in your portfolio → Jump to position detail
- If it's in your watchlist → Jump to watchlist item
- If it's neither → Show asset detail sheet with "Add to Watchlist" and "Add Transaction" buttons

**Search scope:**
- All symbols in `asset_names.rs` (130+ built-in)
- All user's held/watched symbols
- Future: Search via Yahoo Finance autocomplete API for any ticker

---

### Data Entry (Transactions on Mobile)

**Controversial opinion: Don't build a full transaction editor on mobile.**

**Why:**
- Transaction management is power-user work (reviewing cost basis, adjusting dates, bulk import/export)
- Mobile keyboards are slow and error-prone for numerical entry
- The TUI and CLI already handle this perfectly

**What to build instead:**
- **Quick-add transaction** — Tap "Add Transaction" from position detail → Simple form: Buy/Sell picker, quantity, price, date (defaults to today), submit. That's it. No notes, no advanced options.
- **Recent transactions list** — Read-only view of last 10 transactions. Tap to view details. No editing.
- **Bulk import deferred to desktop** — If user needs to import 50 transactions from a CSV, they use the CLI: `pftui import data.json`

**Form design:**

```
┌─────────────────────────────────────┐
│  Add Transaction              [×]   │
│                                      │
│  Symbol:  BTC                        │
│                                      │
│  Type:    [Buy] [Sell]              │
│                                      │
│  Quantity:  [        ]  BTC          │
│                                      │
│  Price:     [        ]  USD          │
│                                      │
│  Date:      Mar 4, 2026  [📅]       │
│                                      │
│  [Cancel]              [Add]         │
└─────────────────────────────────────┘
```

**Numeric input optimizations:**
- Large touch targets for number keys
- Autofocus on Quantity field
- Tab key moves to next field (if user has external keyboard paired)
- Auto-populate price with current market price (user can override)

---

### Gestures

| Gesture | Action |
|---------|--------|
| **Pull down** (on Positions/Watchlist/Macro) | Refresh prices |
| **Swipe left on card** | Delete (with confirmation prompt) |
| **Long press on card** | Quick actions menu (Add transaction, View chart, Remove) |
| **Pinch zoom** (on chart) | Zoom in/out (TradingView handles this) |
| **Pan** (on chart) | Scroll through history (TradingView handles this) |
| **Tap outside modal** | Close modal/sheet |

**No swipe-between-tabs** — Bottom nav is the primary navigation. Swiping between tabs is non-discoverable and conflicts with swipe-to-delete on cards.

---

### Privacy Mode on Mobile

**Toggle:** Tap the 👁️ icon in header (same as TUI's `p` key)

**When enabled:**
- All dollar amounts become "•••••"
- All percentages remain visible (privacy mode in TUI already works this way)
- Allocation donut shows percentages but no $ labels
- Persists across sessions (stored in localStorage or SQLite)

**Use case:** Checking portfolio in public (on train, in coffee shop) without revealing your holdings.

---

## 4. Technical Implementation

### Backend: Axum REST API (Already Exists)

The good news: `/root/pftui/src/web/api.rs` already defines the REST API endpoints we need.

**Existing endpoints (from api.rs scan):**
- `GET /api/portfolio` — Full portfolio summary
- `GET /api/positions` — Position list
- `GET /api/watchlist` — Watchlist items
- `GET /api/transactions` — Transaction history
- `GET /api/macro` — Macro indicators
- `GET /api/alerts` — Alert list
- `GET /api/chart/:symbol` — Price history for charts

**What's missing (needs to be added):**
- `POST /api/transaction` — Add new transaction
- `DELETE /api/transaction/:id` — Remove transaction
- `POST /api/watchlist` — Add symbol to watchlist
- `DELETE /api/watchlist/:symbol` — Remove from watchlist
- `POST /api/alerts` — Create alert
- `PATCH /api/alerts/:id` — Acknowledge/dismiss alert
- `GET /api/search?q=tesla` — Symbol search (autocomplete)

**Auth strategy (Phase 2):**
- **MVP (local only):** No auth. Users run `pftui web` on localhost. Only they have access.
- **Phase 2 (remote/cloud):** Bearer token auth. User runs `pftui web --token` which generates a random token. Client stores token in localStorage. All API requests include `Authorization: Bearer <token>`.
- **Phase 3 (biometric):** WebAuthn for face/fingerprint unlock. Token is stored in browser credential manager.

**API versioning:** All endpoints under `/api/v1/...` to allow future breaking changes without disrupting existing clients.

---

### Frontend: Vanilla JS + Preact + Tailwind CSS

**Framework choice: Preact (not React)**

**Why Preact:**
- Tiny bundle (3kb vs React's 40kb) — crucial for mobile 3G performance
- Exact same API as React (easy to hire devs, ChatGPT knows it)
- Fast enough for our needs (we're not building a realtime trading platform)
- No build complexity (Vite + Preact = instant HMR)

**Why NOT React:**
- Overkill for a portfolio dashboard
- Bigger bundle = slower first paint on mobile
- React Server Components would be wasted (we have an API, not SSR)

**Why NOT Alpine.js / htmx:**
- Alpine is great for sprinkling interactivity on server-rendered pages. We're building a SPA.
- htmx is brilliant for hypermedia-driven apps. But we already have a REST API and need client-side state (offline caching, optimistic updates).

**Why NOT Vue / Svelte:**
- Preact is more popular (better hiring, more examples, ChatGPT is better at it)
- Svelte's compiler magic is elegant but less debuggable
- Vue's ecosystem is smaller for financial/chart components

**CSS framework: Tailwind CSS**

**Why Tailwind:**
- Fastest way to build responsive layouts (mobile-first by default)
- Purges unused styles = tiny production CSS bundle
- Works perfectly with component-based JS (no CSS-in-JS overhead)
- Industry standard (every modern webapp uses it)

**Component structure:**

```
src/web/static/
  js/
    components/
      PositionCard.jsx
      AllocationDonut.jsx
      PriceChart.jsx       // TradingView wrapper
      BottomNav.jsx
      Header.jsx
      AlertBadge.jsx
      SearchOverlay.jsx
      TransactionForm.jsx
    pages/
      Positions.jsx
      Markets.jsx
      Macro.jsx
      Watchlist.jsx
      Journal.jsx
    lib/
      api.js               // fetch() wrappers for REST API
      auth.js              // token management
      storage.js           // localStorage + IndexedDB helpers
    App.jsx
    main.jsx
  css/
    app.css                // Tailwind imports
  index.html
  manifest.json            // PWA manifest
  sw.js                    // Service worker
```

**State management:** Preact signals (built-in, no Redux needed). Global state:
- `portfolioData` — positions, total value, allocation
- `watchlistData` — watched symbols
- `macroData` — market indicators
- `alertsData` — triggered alerts
- `authToken` — bearer token for API calls
- `offlineMode` — boolean (true when network is down)

---

### Offline Strategy (PWA)

**Service worker caching strategy:**

1. **App shell (cache-first)** — HTML, CSS, JS bundles. Cached on install, updated on version bump.
2. **Price data (stale-while-revalidate)** — Show cached prices immediately, fetch fresh in background, update when ready.
3. **Chart data (cache-first with TTL)** — Price history doesn't change retroactively. Cache for 24 hours.
4. **User mutations (queue in IndexedDB)** — If offline, queue POST/DELETE requests (add transaction, add to watchlist) in IndexedDB. Replay when back online.

**Offline experience:**

```
┌─────────────────────────────────────┐
│  pftui                   ⚠️ Offline │
│                                      │
│  Portfolio (as of 2h ago)            │
│  $368,300                            │
│                                      │
│  Prices may be outdated.             │
│  Last updated: 2:30 PM               │
│                                      │
│  [Retry Connection]│                                      │
│  [Your positions appear normally]    │
└─────────────────────────────────────┘
```

**Background sync API** (Android only): When network reconnects, service worker automatically syncs queued mutations (added transactions, watchlist changes). User sees a toast: "✓ Synced 3 offline changes."

**Storage limits:**
- iOS Safari: ~50MB before prompting user
- Android Chrome: ~1GB available (graceful degradation if exceeded)
- Strategy: Keep only 90 days of price history cached. Purge older data automatically.

---

### Real-Time Updates

**Problem:** Portfolio value changes as prices update. How to keep the UI fresh?

**Options:**

1. **WebSocket** — Server pushes price updates to client in real-time.
   - **Pro:** Instant updates, low latency
   - **Con:** More complex server code, doesn't work offline, connection overhead

2. **Server-Sent Events (SSE)** — Server streams price updates to client.
   - **Pro:** Simpler than WebSocket, works over HTTP
   - **Con:** Unidirectional (can't send data back), Safari support is iffy

3. **Polling** — Client fetches `/api/portfolio` every N seconds.
   - **Pro:** Dead simple, works everywhere, works offline (fails gracefully)
   - **Con:** Higher bandwidth, slower updates

**Recommendation: Polling with smart backoff**

- When app is in foreground: poll every 30 seconds
- When app is in background: poll every 5 minutes (or stop entirely)
- When offline: stop polling, resume when network reconnects
- Use HTTP `If-Modified-Since` to avoid re-fetching unchanged data

**Why not WebSocket:** Overkill for a portfolio tracker. Prices don't change THAT fast (we're fetching from Yahoo/CoinGecko every 60 seconds anyway). Polling every 30 seconds is indistinguishable from "real-time" for this use case.

**Future optimization:** If user has the app open for >1 hour, switch to SSE to reduce bandwidth. But start with polling — it's simpler and works everywhere.

---

### Authentication on Mobile

**Phase 1 (MVP): Local-only, no auth**

User runs `pftui web` on their laptop. Webapp is accessible at `http://localhost:3000`. Only works on the same machine. No auth needed.

**Phase 2: Remote access with bearer token**

User runs `pftui web --bind 0.0.0.0 --token`. Server generates a random 32-char token and prints it:

```
pftui web server started
URL: http://192.168.1.100:3000
Token: g8jK2nP9qR3sT5vW7xY0zA1bC4dE6fH9
```

User copies the URL and token, opens it on their phone. Webapp prompts for token on first visit. Token is stored in localStorage. All subsequent API requests include `Authorization: Bearer <token>`.

**Security:**
- Token is single-use per device (once used, can't be reused on another device without re-running `--token`)
- Token expires after 30 days of inactivity
- User can revoke tokens via `pftui web --revoke-tokens`

**Phase 3: Biometric unlock (WebAuthn)**

User registers their fingerprint/face via WebAuthn. Token is stored in browser's credential manager (encrypted, not extractable). On app launch, biometric prompt appears. After unlock, token is retrieved and used for API calls.

**Why not password:** Typing passwords on mobile is painful. Biometrics are faster and more secure (can't be phished).

---

### Performance Targets

| Metric | Target | Why |
|--------|--------|-----|
| **First Contentful Paint** | <1s on 3G | User sees something instantly |
| **Time to Interactive** | <2s on 3G | App is usable within 2 seconds |
| **JS bundle size** | <100kb gzipped | Fast download on slow networks |
| **API response time** | <200ms (local) | Feels instant |
| **Offline app load** | <500ms | Service worker cache is fast |

**Optimization strategies:**

1. **Code splitting** — Load Positions page first, lazy-load Journal/Macro tabs on demand
2. **Image optimization** — Use WebP for icons/logos, SVG for charts
3. **Prefetching** — When user hovers over a tab, prefetch that tab's data
4. **Compression** — Gzip all text assets (HTML/CSS/JS), Brotli if server supports it
5. **CDN for static assets** — If self-hosting, serve static files from a CDN (optional)

**Monitoring:** Use Lighthouse CI in GitHub Actions to fail builds that regress performance scores below 90.

---

## 5. Phased Rollout

### Phase 1: MVP (Read-Only Portfolio Dashboard)

**Goal:** Prove the concept. Ship a mobile-friendly portfolio viewer in 40 hours.

**Features:**
- ✅ Bottom tab navigation (Positions, Markets, Macro, Watchlist)
- ✅ Position cards with sparklines
- ✅ Total portfolio value + daily P&L
- ✅ Allocation donut chart
- ✅ Macro indicators (SPX, VIX, Gold, 10Y, DXY)
- ✅ Watchlist with price targets
- ✅ Position detail sheet with mini chart
- ✅ Privacy mode toggle
- ✅ Responsive layout (works on desktop too)
- ✅ Pull-to-refresh

**NOT in Phase 1:**
- ❌ Service worker / offline mode
- ❌ TradingView charts (use simple SVG line charts)
- ❌ Add transaction / data entry
- ❌ Alerts
- ❌ Journal
- ❌ Authentication

**Success criteria:**
- User can check their portfolio on mobile in <3 seconds
- UI feels smooth (60fps scroll, instant taps)
- Works on iPhone Safari and Android Chrome

**Timeframe:** 2 weeks (40 hours)

---

### Phase 2: Interactivity & Charts

**Goal:** Make it useful for decision-making, not just viewing.

**Features:**
- ✅ TradingView embedded charts (full-screen on tap)
- ✅ Search overlay (find any symbol, not just held/watched)
- ✅ Add to watchlist from search
- ✅ Quick-add transaction (simple form)
- ✅ Long-press quick actions menu
- ✅ Swipe-to-delete on positions/watchlist

**Success criteria:**
- User can research a symbol and add it to watchlist without using TUI
- User can add a transaction on mobile (even if it's not the primary workflow)

**Timeframe:** 1 week (20 hours)

---

### Phase 3: Offline & PWA

**Goal:** Make it feel like a native app.

**Features:**
- ✅ Service worker with offline caching
- ✅ "Add to Home Screen" prompt
- ✅ App manifest (icon, splash screen, theme color)
- ✅ Offline indicator + queued mutations
- ✅ Background sync when network reconnects

**Success criteria:**
- User can open the app on a plane and see yesterday's prices
- App loads in <500ms when offline
- User can add transactions offline and they sync when network returns

**Timeframe:** 1 week (20 hours)

---

### Phase 4: Alerts & Notifications

**Goal:** Make it proactive. The app tells you when something important happens.

**Features:**
- ✅ Alert creation UI (price targets, allocation drift, indicator thresholds)
- ✅ Alert list with triggered/active states
- ✅ In-app badge count
- ✅ Web Push notifications (Android)
- ✅ Email alerts (fallback for users who don't want push)

**Success criteria:**
- User sets "Notify when BTC > $75k" and gets a push notification when it hits
- User sees alert badge in header when something triggered while app was closed

**Timeframe:** 1 week (20 hours)

---

### Phase 5: Journal & Full Feature Parity

**Goal:** Complete the mobile experience. Everything you need, nothing you don't.

**Features:**
- ✅ Journal tab (recent entries, search, quick-add)
- ✅ Correlation matrix (simplified for mobile)
- ✅ Scenario modeling ("what if BTC drops 20%?")
- ✅ Settings panel (theme, base currency, sync)

**Success criteria:**
- User can log a trade thesis from mobile
- User can run a quick scenario without opening the TUI

**Timeframe:** 2 weeks (40 hours)

---

### Phase 6 (Future): Native Wrapper (Optional)

**Goal:** App store presence for discoverability.

**Features:**
- ✅ Capacitor wrapper for iOS and Android
- ✅ Native biometric auth
- ✅ Native push notifications
- ✅ App store submissions

**Success criteria:**
- App is available in Apple App Store and Google Play Store
- Native features work (Face ID, native notifications)

**Timeframe:** 3 weeks (60 hours)

**Decision point:** Only pursue this if user feedback demands it. PWA may be sufficient.

---

## 6. Wireframes (ASCII)

### Portfolio Home (375px width)

```
┌───────────────────────────────────────┐
│ pftui                      🔔 👁️ ⚙️  │
├───────────────────────────────────────┤
│                                        │
│   Total Portfolio                      │
│   $368,300.00                          │
│   +$4,230 (+1.16%) ↗️                  │
│                                        │
│   ┌─────────────────────────────────┐ │
│   │    [Allocation Donut Chart]      │ │
│   │   Cash 49% · Commodity 31%       │ │
│   │   Crypto 20%                     │ │
│   └─────────────────────────────────┘ │
│                                        │
│   Positions ──────────────────────    │
│                                        │
│ ┌────────────────────────────────────┐│
│ │ 💵 US Dollar (USD)          ---    ││
│ │ $1.00                              ││
│ │ ─────────── (cash)                 ││
│ │ 179,420 USD · $179,420 · 48.7%    ││
│ │ Gain: $0 (0.0%) ─                  ││
│ └────────────────────────────────────┘│
│                                        │
│ ┌────────────────────────────────────┐│
│ │ 🪙 Gold (GC=F)             -3.0%   ││
│ │ $5,139.00                          ││
│ │ ▅▆▇▆▅▄▃▄▅▆▇ 30D                   ││
│ │ 17.87 oz · $91,833 · 24.9%        ││
│ │ Gain: -$1,234 (-1.3%) ✗            ││
│ └────────────────────────────────────┘│
│                                        │
│ ┌────────────────────────────────────┐│
│ │ 🪙 Bitcoin (BTC)           +4.1%   ││
│ │ $73,705.00                         ││
│ │ ▂▃▅▆▇▅▃▂▁▃▅ 30D                   ││
│ │ 0.99 BTC · $73,568 · 20.0%        ││
│ │ Gain: +$3,220 (+4.6%) ✓            ││
│ └────────────────────────────────────┘│
│                                        │
│ ┌────────────────────────────────────┐│
│ │ 🥈 Silver (SI=F)           -4.9%   ││
│ │ $83.64                             ││
│ │ ▇▆▅▄▃▄▃▂▃▄▅ 30D                   ││
│ │ 270 oz · $22,582 · 6.1%           ││
│ │ Gain: -$890 (-3.8%) ✗              ││
│ └────────────────────────────────────┘│
│                                        │
├───────────────────────────────────────┤
│   📊     📈     🌍     👁️     📝      │
│ Position Market Macro Watch Journal   │
└───────────────────────────────────────┘
```

---

### Position Detail with Chart

```
┌───────────────────────────────────────┐
│        Bitcoin (BTC)             [×]  │
├───────────────────────────────────────┤
│                                        │
│         $73,705.00                     │
│         +$3,012 (+4.1%)               │
│                                        │
│  ┌──────────────────────────────────┐ │
│  │ 74k ┤                          ╭─ │ │
│  │     │                      ╭───╯  │ │
│  │ 72k ┤                  ╭───╯      │ │
│  │     │              ╭───╯          │ │
│  │ 70k ┼──────────────╯              │ │
│  │     └──────────────────────────── │ │
│  │     Jan 5      Feb 3      Mar 4   │ │
│  │           30-Day Chart             │ │
│  └──────────────────────────────────┘ │
│                                        │
│  Position ───────────────────────     │
│  Quantity:       0.99 BTC              │
│  Avg Cost:       $70,272               │
│  Total Cost:     $69,569               │
│  Current Value:  $73,568               │
│  Gain/Loss:      +$3,999 (+5.7%)      │
│  Allocation:     20.0%                 │
│  52W Range:      47%  ▓▓▓▓░░░░  98%  │
│                                        │
│  Technical ───────────────────────     │
│  RSI (14):       56 ⚪ Neutral         │
│  MACD:           Bullish ▲             │
│                                        │
│  Actions ─────────────────────────     │
│  [View Full Chart]  [Add Transaction]  │
│  [Remove Position]                     │
│                                        │
└───────────────────────────────────────┘
```

---

### Watchlist

```
┌───────────────────────────────────────┐
│ Watchlist                          +  │
├───────────────────────────────────────┤
│                                        │
│ ┌────────────────────────────────────┐│
│ │ TSLA                        -2.3%  ││
│ │ Tesla Inc.                         ││
│ │ $285.40                            ││
│ │ Target: $300 ▲ (+5.1% away)       ││
│ │ ▄▅▆▇▆▅▄▃▃▄▅ 30D                   ││
│ └────────────────────────────────────┘│
│                                        │
│ ┌────────────────────────────────────┐│
│ │ AAPL                        +1.2%  ││
│ │ Apple Inc.                         ││
│ │ $182.50                            ││
│ │ Target: $175 ▼ (hit!)  🎯         ││
│ │ ▂▃▄▅▆▇▆▅▄▃▂ 30D                   ││
│ └────────────────────────────────────┘│
│                                        │
│ ┌────────────────────────────────────┐│
│ │ GC=F                        -0.8%  ││
│ │ Gold Futures                       ││
│ │ $2,165.00                          ││
│ │ No target set                      ││
│ │ ▇▆▅▄▃▄▅▆▇▆▅ 30D                   ││
│ └────────────────────────────────────┘│
│                                        │
├───────────────────────────────────────┤
│   📊     📈     🌍     👁️     📝      │
│ Position Market Macro Watch Journal   │
└───────────────────────────────────────┘
```

---

### Macro Dashboard

```
┌───────────────────────────────────────┐
│ Macro                                  │
├───────────────────────────────────────┤
│                                        │
│ 📊 Equities ──────────────────────    │
│                                        │
│  SPX        5,234     +0.3%    ▃▅▇    │
│  NDX       18,401     +0.5%    ▃▆▇    │
│  DJI       41,203     +0.1%    ▂▃▄    │
│  BTC       73,705     +4.1%    ▂▅▇    │
│                                        │
│ 💰 Commodities ───────────────────    │
│                                        │
│  Gold       2,165     -0.8%    ▇▅▃    │
│  Silver      25.3     -1.5%    ▇▄▂    │
│  Oil         78.4     +1.2%    ▂▄▆    │
│  Copper      4.82     -0.3%    ▅▄▃    │
│                                        │
│ 📈 Rates & Volatility ────────────    │
│                                        │
│  10Y Yield   4.23%    +2bps    ▂▃▅    │
│  2Y Yield    4.18%    +1bp     ▂▃▄    │
│  VIX        14.30     -5.0%    ▅▃▂    │
│  DXY       104.20     +0.4%    ▂▃▄    │
│                                        │
│ 🌍 Currencies ────────────────────    │
│                                        │
│  EUR/USD     1.082    -0.2%    ▄▃▂    │
│  USD/JPY   150.30     +0.1%    ▂▃▃    │
│  GBP/USD     1.267    -0.1%    ▃▃▂    │
│                                        │
├───────────────────────────────────────┤
│   📊     📈     🌍     👁️     📝      │
│ Position Market Macro Watch Journal   │
└───────────────────────────────────────┘
```

---

### Search Overlay

```
┌───────────────────────────────────────┐
│ [🔍 Search symbol or name...]    [×] │
├───────────────────────────────────────┤
│                                        │
│ Recent:                                │
│ TSLA · AAPL · BTC · GC=F              │
│                                        │
│ ───────────────────────────────────── │
│                                        │
│ (User types "silv")                    │
│                                        │
│ Results:                               │
│                                        │
│ ┌────────────────────────────────────┐│
│ │ SI=F                                ││
│ │ Silver Futures              Commodity││
│ │ $83.64     -4.9%                   ││
│ └────────────────────────────────────┘│
│                                        │
│ ┌────────────────────────────────────┐│
│ │ SLV                                 ││
│ │ iShares Silver Trust             ETF││
│ │ $25.30     -1.8%                   ││
│ └────────────────────────────────────┘│
│                                        │
│ ┌────────────────────────────────────┐│
│ │ U.UN                                ││
│ │ Sprott Physical Silver         Fund ││
│ │ CAD 20.17  -4.0%                   ││
│ └────────────────────────────────────┘│
│                                        │
└───────────────────────────────────────┘
```

---

### Alert List

```
┌───────────────────────────────────────┐
│ Alerts                           [×]  │
├───────────────────────────────────────┤
│                                        │
│ ┌────────────────────────────────────┐│
│ │ 🔴 Bitcoin above $75,000            ││
│ │    Triggered 2h ago                 ││
│ │    Current: $75,204                 ││
│ │    [Dismiss]                        ││
│ └────────────────────────────────────┘│
│                                        │
│ ┌────────────────────────────────────┐│
│ │ 🟡 Gold allocation above 30%        ││
│ │    Triggered today                  ││
│ │    Current: 31.2%                   ││
│ │    [Dismiss]                        ││
│ └────────────────────────────────────┘│
│                                        │
│ ┌────────────────────────────────────┐│
│ │ 🟢 VIX below 15                     ││
│ │    Triggered 6h ago                 ││
│ │    Current: 14.3                    ││
│ │    [Dismiss]                        ││
│ └────────────────────────────────────┘│
│                                        │
│ Active Alerts (3) ────────────────    │
│                                        │
│  • BTC > $80,000                       │
│  • Gold alloc < 25%                    │
│  • VIX > 20                            │
│                                        │
│  [Manage Alerts]                       │
│                                        │
└───────────────────────────────────────┘
```

---

### Settings

```
┌───────────────────────────────────────┐
│ Settings                         [×]  │
├───────────────────────────────────────┤
│                                        │
│ Appearance ──────────────────────     │
│                                        │
│  Theme:         [Midnight ▼]           │
│  Privacy Mode:  [OFF]  (toggle)        │
│                                        │
│ Data ────────────────────────────     │
│                                        │
│  Base Currency: [USD ▼]                │
│  Sync:          [Disabled]             │
│                  [Configure Sync...]   │
│                                        │
│ Notifications ───────────────────     │
│                                        │
│  Push Alerts:   [ON]   (toggle)        │
│  Email Alerts:  [OFF]  (toggle)        │
│  Email:         [skylar@...]           │
│                                        │
│ Advanced ────────────────────────     │
│                                        │
│  Server URL:    localhost:3000         │
│  Auth Token:    ••••••••••••••         │
│                 [Change Token]         │
│  Cache Size:    24 MB                  │
│                 [Clear Cache]          │
│                                        │
│ About ───────────────────────────     │
│                                        │
│  Version:       0.4.0                  │
│  License:       MIT                    │
│  [View on GitHub]                      │
│  [Documentation]                       │
│                                        │
└───────────────────────────────────────┘
```

---

## 7. Open Questions

These are decisions that need Skylar's input before implementation:

### 1. Sync Strategy: Local-Only vs Cloud-Synced Database?

**Local-only:**
- User runs `pftui web` on their laptop/desktop
- Mobile webapp connects to `http://192.168.1.100:3000` (local network only)
- Portfolio stays on user's machine, zero cloud involvement

**Cloud-synced:**
- User's SQLite database syncs to a self-hosted server (Dropbox, iCloud, or custom S3-like storage)
- Mobile webapp connects to the sync server
- Portfolio is accessible from anywhere (not just local network)

**Tradeoff:**
- Local-only is more secure, zero privacy concerns, no sync complexity
- Cloud-synced is more convenient, works away from home, but requires sync infrastructure

**Question:** Which is the priority for v1? Local-only is simpler. Cloud sync can be Phase 3.

---

### 2. TradingView License: Free vs Paid?

**Free TradingView embed:**
- Includes "Powered by TradingView" branding
- Limited to 5 widgets per page
- No commercial use (pftui is MIT-licensed open source, so this is gray area)

**Paid TradingView license ($250-500/year):**
- Remove branding
- Unlimited widgets
- Explicit commercial use allowed

**Question:** Are we OK with TradingView branding? If pftui becomes popular, we might need to pay for the license. Alternative: build our own interactive charts (heavy lift, ~40 hours).

---

### 3. Should Journal Support Voice Memos?

Many mobile finance apps (Robinhood, Fidelity) let you attach voice memos to trades. Example: "I'm buying TSLA because I think FSD will ship next quarter" recorded as audio, transcribed to text later.

**Tradeoff:**
- Cool feature, very mobile-native
- Adds complexity (audio recording API, storage, transcription)
- Transcription requires either client-side ML (Whisper.cpp) or cloud API ($$)

**Question:** Is voice memo journaling worth the complexity? Or defer to Phase 5+?

---

### 4. Multi-Portfolio Support on Mobile?

The TUI doesn't currently support multiple portfolios (you have one SQLite database per instance). But mobile users might want:
- "Personal" portfolio
- "401k" portfolio (read-only, just for tracking)
- "Demo" portfolio (play money)

**Tradeoff:**
- More complex UI (portfolio switcher in header)
- Database schema changes (multi-tenancy)
- Sync complexity (which portfolio to sync?)

**Question:** Is multi-portfolio a Phase 1 requirement, or defer to post-MVP?

---

### 5. What's the Home Screen Default?

Two schools of thought:

**A) Positions (portfolio-first)** — Most users track a portfolio. Show total value + positions by default.

**B) Macro (market-first)** — Some users want to see "what's happening in the world" before checking their holdings. Open to Macro tab by default.

**Question:** Which should be the default landing page? (Can make it configurable later, but need a sensible default.)

---

### 6. Should We Support Dark Mode Only, or Light Mode Too?

The TUI has 6 dark themes. Mobile apps often support light mode for outdoor visibility.

**Tradeoff:**
- Dark-only is simpler (one set of colors to design)
- Light mode improves readability in sunlight
- Finance apps (Robinhood, Yahoo) default to dark but offer light

**Question:** Dark-only for MVP, or build light mode from the start?

---

### 7. How Aggressive Should Offline Caching Be?

**Conservative:** Cache only 7 days of price history. App is lightweight, but limited offline utility.

**Aggressive:** Cache 90 days of price history + all macro indicators. Larger bundle, but fully functional offline.

**Question:** What's the right balance? (Can make it configurable, but need a default.)

---

### 8. Transaction Entry: Required Feature or "Nice to Have"?

**Argument FOR:** Users expect to add transactions from mobile. Feels incomplete without it.

**Argument AGAINST:** Transaction entry is slow on mobile keyboards. The TUI and CLI are better for this. Mobile should be read-only + quick actions (watchlist, alerts).

**Question:** Is transaction entry a Phase 1 must-have, or Phase 2 feature?

---

### 9. Should We Gamify Anything?

Some portfolio apps (Robinhood, Acorns) use gamification: streak counters, achievement badges, confetti animations.

Examples:
- "🔥 7-day portfolio check streak"
- "🎯 Achievement unlocked: First $100k portfolio"
- "🎉 Confetti when portfolio hits new all-time high"

**Tradeoff:**
- Fun, increases engagement
- Can feel gimmicky for serious investors (pftui is NOT Robinhood)

**Question:** Should pftui lean into gamification, or stay strictly utilitarian?

---

### 10. International Users: Multi-Currency Display?

pftui supports `base_currency` in config, but the webapp could:
- Auto-detect user's locale (browser language)
- Display prices in local currency (USD → EUR conversion on-the-fly)
- Show local stock exchanges (Frankfurt, London, Tokyo)

**Tradeoff:**
- More inclusive, better international UX
- Adds complexity (FX conversion, locale handling, exchange mapping)

**Question:** Is international support a Phase 1 goal, or US-first and expand later?

---

## Conclusion

The mobile webapp is **NOT** a TUI port. It's a purpose-built mobile experience optimized for quick checks, tactical research, and lightweight data entry.

**Key principles:**
1. **Speed first** — User sees their portfolio in <2 seconds, even on 3G
2. **Offline-capable** — Works on planes, trains, rural areas
3. **Privacy-preserving** — Local-first, no third-party tracking
4. **Touch-optimized** — Cards, bottom sheets, gestures feel native
5. **Macro-aware** — Not just "here's your balance" — shows market context

**Recommended path:**
1. Ship PWA (Option A) as MVP
2. Start with read-only dashboard (Phase 1)
3. Add interactivity + charts (Phase 2)
4. Add offline mode (Phase 3)
5. Decide on native wrapper (Option D) based on user demand

**Total effort estimate:** 140 hours to full-featured PWA (Phases 1-5). Native wrapper adds 60 more hours.

**Next step:** Get Skylar's input on Open Questions, then start with Phase 1 (40 hours, 2 weeks).

---

**End of Document**
