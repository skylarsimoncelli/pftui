import { Page, Route } from "@playwright/test";

const THEMES = [
  "midnight",
  "catppuccin",
  "nord",
  "dracula",
  "solarized",
  "gruvbox",
  "inferno",
  "neon",
  "hacker",
  "pastel",
  "miasma",
];

function meta() {
  return {
    last_refresh_at: "2026-03-05T20:00:00Z",
    stale_after_sec: 60,
    source_status: "ok",
    auth_required: true,
    transport: "polling",
  };
}

function routeJson(route: Route, body: unknown) {
  return route.fulfill({
    status: 200,
    contentType: "application/json",
    body: JSON.stringify(body),
  });
}

export async function installWebMocks(page: Page) {
  await page.route("https://s3.tradingview.com/tv.js", async (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/javascript",
      body: "window.TradingView={widget:function(){return {remove:function(){}}}};",
    }),
  );

  await page.route("**/auth/session", async (route) =>
    routeJson(route, {
      authenticated: true,
      issued_at: "2026-03-05T20:00:00Z",
      expires_at: "2026-03-06T04:00:00Z",
      csrf_token: "csrf_test",
      auth_mode: "session",
    }),
  );
  await page.route("**/auth/csrf", async (route) =>
    routeJson(route, { csrf_token: "csrf_test" }),
  );
  await page.route("**/auth/login", async (route) =>
    routeJson(route, {
      ok: true,
      issued_at: "2026-03-05T20:00:00Z",
      expires_at: "2026-03-06T04:00:00Z",
      csrf_token: "csrf_test",
      auth_mode: "session",
    }),
  );
  await page.route("**/auth/logout", async (route) => routeJson(route, { ok: true }));

  await page.route("**/api/ui-config", async (route) =>
    routeJson(route, {
      tabs: [
        "Positions",
        "Transactions",
        "Markets",
        "Economy",
        "Watchlist",
        "Alerts",
        "News",
        "Journal",
      ],
      themes: THEMES.map((name) => ({
        name,
        colors: {
          bg_primary: "#0d1117",
          bg_secondary: "#161b22",
          bg_tertiary: "#21262d",
          text_primary: "#c9d1d9",
          text_secondary: "#8b949e",
          text_muted: "#6e7681",
          text_accent: "#89dceb",
          border: "#30363d",
          accent: "#89b4fa",
          green: "#a6e3a1",
          red: "#f38ba8",
          yellow: "#f9e2af",
        },
      })),
      current_theme: "midnight",
      home_tab: "positions",
    }),
  );

  await page.route("**/api/portfolio", async (route) =>
    routeJson(route, {
      total_value: "125000",
      total_cost: "100000",
      total_gain: "25000",
      total_gain_pct: "25",
      daily_change: "1200",
      daily_change_pct: "0.97",
      positions: [
        {
          symbol: "AAPL",
          name: "Apple Inc",
          category: "equity",
          quantity: "10",
          avg_cost: "150",
          total_cost: "1500",
          currency: "USD",
          current_price: "200",
          current_value: "2000",
          gain: "500",
          gain_pct: "33.33",
          allocation_pct: "8.2",
        },
        {
          symbol: "BTC",
          name: "Bitcoin",
          category: "crypto",
          quantity: "1",
          avg_cost: "45000",
          total_cost: "45000",
          currency: "USD",
          current_price: "68000",
          current_value: "68000",
          gain: "23000",
          gain_pct: "51.11",
          allocation_pct: "41.5",
        },
      ],
      meta: meta(),
    }),
  );

  await page.route("**/api/watchlist", async (route) =>
    routeJson(route, {
      symbols: [
        {
          symbol: "TSLA",
          name: "Tesla",
          category: "equity",
          current_price: "240",
          day_change_pct: "1.2",
          target_price: "260",
          target_direction: "above",
          distance_pct: "7.69",
          target_hit: false,
        },
      ],
      meta: meta(),
    }),
  );

  await page.route("**/api/macro", async (route) =>
    routeJson(route, {
      indicators: [
        { symbol: "^GSPC", name: "S&P 500", value: "5100", change_pct: "0.8" },
        { symbol: "DX-Y.NYB", name: "DXY", value: "104.2", change_pct: "-0.2" },
      ],
      sections: [
        {
          id: "macro",
          label: "Macro",
          indicators: [{ symbol: "GC=F", name: "Gold", value: "2200", change_pct: "0.6" }],
        },
      ],
      top_movers: [],
      meta: meta(),
    }),
  );

  await page.route("**/api/transactions**", async (route) =>
    routeJson(route, {
      transactions: [
        {
          id: 1,
          symbol: "AAPL",
          tx_type: "buy",
          quantity: "10",
          price_per: "150",
          date: "2026-03-01",
        },
      ],
      sort_by: "date",
      sort_order: "desc",
      meta: meta(),
    }),
  );

  await page.route("**/api/performance**", async (route) =>
    routeJson(route, {
      daily_values: [
        { date: "2026-03-01", value: "100000" },
        { date: "2026-03-02", value: "102000" },
        { date: "2026-03-03", value: "105000" },
      ],
      metrics: { total_return_pct: "5", max_drawdown_pct: "-2.2" },
      estimated: false,
      coverage_pct: "100",
      source: "snapshots",
      benchmark_values: [],
      meta: meta(),
    }),
  );

  await page.route("**/api/summary", async (route) =>
    routeJson(route, {
      total_value: "125000",
      position_count: 2,
      top_movers: [
        {
          symbol: "AAPL",
          name: "Apple Inc",
          category: "equity",
          quantity: "10",
          avg_cost: "150",
          total_cost: "1500",
          currency: "USD",
          current_price: "200",
          current_value: "2000",
          gain: "500",
          gain_pct: "33.33",
          allocation_pct: "8.2",
        },
      ],
      meta: meta(),
    }),
  );

  await page.route("**/api/alerts", async (route) =>
    routeJson(route, {
      alerts: [
        {
          id: 1,
          kind: "price",
          symbol: "AAPL",
          direction: "above",
          threshold: "210",
          rule_text: "AAPL > 210",
          status: "active",
          triggered_at: null,
        },
      ],
      meta: meta(),
    }),
  );

  await page.route("**/api/news**", async (route) =>
    routeJson(route, {
      entries: [
        {
          id: 1,
          title: "Apple gains on strong iPhone demand",
          url: "https://example.com/apple",
          source: "Reuters",
          category: "Markets",
          published_at: 1700000000,
          fetched_at: "2026-03-05T19:55:00Z",
        },
      ],
      meta: meta(),
    }),
  );

  await page.route("**/api/journal**", async (route) =>
    routeJson(route, {
      entries: [
        {
          id: 1,
          symbol: "AAPL",
          status: "open",
          tag: "swing",
          conviction: 4,
          content: "Watching support at 190",
          timestamp: "2026-03-05T19:55:00Z",
        },
      ],
      meta: meta(),
    }),
  );

  await page.route("**/api/chart/**", async (route) =>
    routeJson(route, {
      symbol: "AAPL",
      history: [
        { date: "2026-03-01", close: "198", volume: 1_000_000 },
        { date: "2026-03-02", close: "199", volume: 1_100_000 },
        { date: "2026-03-03", close: "200", volume: 1_200_000 },
      ],
      meta: meta(),
    }),
  );

  await page.route("**/api/home-tab", async (route) => routeJson(route, { ok: true, home_tab: "positions" }));
  await page.route("**/api/theme", async (route) => routeJson(route, { ok: true, theme: "midnight" }));
  await page.route("**/api/stream", async (route) =>
    route.fulfill({
      status: 200,
      contentType: "text/event-stream",
      body: "event: heartbeat\ndata: {\"ts\":\"2026-03-05T20:00:00Z\",\"message\":\"alive\"}\n\n",
    }),
  );
}
