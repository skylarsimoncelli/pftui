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
  const watchlistEntries = [
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
  ];
  const alerts = [
    {
      id: 1,
      kind: "price",
      symbol: "AAPL",
      direction: "above",
      threshold: "210",
      rule_text: "AAPL above 210",
      status: "triggered",
      triggered_at: "2026-03-05T19:55:00Z",
    },
  ];
  let nextAlertId = 2;
  const transactions = [
    {
      id: 1,
      symbol: "AAPL",
      category: "equity",
      tx_type: "buy",
      quantity: "10",
      price_per: "150",
      currency: "USD",
      date: "2026-03-01",
      notes: null,
    },
  ];
  let nextTxId = 2;
  const journalEntries = [
    {
      id: 1,
      symbol: "AAPL",
      status: "open",
      tag: "swing",
      conviction: 4,
      content: "Watching support at 190",
      timestamp: "2026-03-05T19:55:00Z",
    },
  ];
  let nextJournalId = 2;

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

  await page.route("**/api/watchlist**", async (route) => {
    const method = route.request().method();
    const url = new URL(route.request().url());
    const parts = url.pathname.split("/").filter(Boolean);
    const symbol = decodeURIComponent(parts[parts.length - 1] || "").toUpperCase();

    if (method === "POST") {
      const body = route.request().postDataJSON() as { symbol?: string; category?: string } | null;
      const nextSymbol = body?.symbol?.toUpperCase() || "";
      if (nextSymbol && !watchlistEntries.some((w) => w.symbol === nextSymbol)) {
        watchlistEntries.unshift({
          symbol: nextSymbol,
          name: nextSymbol === "MSFT" ? "Microsoft" : nextSymbol,
          category: body?.category || "equity",
          current_price: "410",
          day_change_pct: "0.6",
          target_price: null,
          target_direction: null,
          distance_pct: null,
          target_hit: false,
        });
      }
      return routeJson(route, { ok: true, symbol: nextSymbol, action: "added" });
    }

    if (method === "DELETE") {
      const idx = watchlistEntries.findIndex((w) => w.symbol === symbol);
      if (idx >= 0) watchlistEntries.splice(idx, 1);
      return routeJson(route, { ok: idx >= 0, symbol, action: idx >= 0 ? "removed" : "noop" });
    }

    return routeJson(route, {
      symbols: watchlistEntries,
      meta: meta(),
    });
  });

  await page.route("**/api/search**", async (route) => {
    const q = (new URL(route.request().url()).searchParams.get("q") || "").toUpperCase();
    const all = [
      { symbol: "AAPL", name: "Apple", category: "equity", current_price: "200", day_change_pct: "1.1" },
      { symbol: "MSFT", name: "Microsoft", category: "equity", current_price: "410", day_change_pct: "0.6" },
      { symbol: "TSLA", name: "Tesla", category: "equity", current_price: "240", day_change_pct: "1.2" },
      { symbol: "BTC", name: "Bitcoin", category: "crypto", current_price: "68000", day_change_pct: "2.0" },
    ];
    const results = all
      .filter((x) => x.symbol.includes(q) || x.name.toUpperCase().includes(q))
      .map((x) => ({
        ...x,
        is_watchlisted: watchlistEntries.some((w) => w.symbol === x.symbol),
      }));
    return routeJson(route, { results, meta: meta() });
  });

  await page.route("**/api/asset/**", async (route) => {
    const symbol = decodeURIComponent(route.request().url().split("/").pop() || "").toUpperCase();
    const inWatchlist = watchlistEntries.some((w) => w.symbol === symbol);
    return routeJson(route, {
      symbol,
      history_symbol: symbol,
      name: symbol === "MSFT" ? "Microsoft" : symbol,
      category: "equity",
      is_watchlisted: inWatchlist,
      alert_count: 1,
      current_price: symbol === "MSFT" ? "410" : "200",
      day_change_pct: "0.8",
      week_change_pct: "1.4",
      month_change_pct: "3.7",
      year_change_pct: "18.2",
      range_52w_low: "280",
      range_52w_high: "430",
      latest_volume: 1250000,
      avg_volume_30d: 1100000,
      position: {
        quantity: symbol === "AAPL" ? "10" : "0",
        current_value: symbol === "AAPL" ? "2000" : null,
        gain: symbol === "AAPL" ? "500" : null,
        gain_pct: symbol === "AAPL" ? "33.33" : null,
        allocation_pct: symbol === "AAPL" ? "8.2" : null,
      },
      history: [
        { date: "2026-03-01", close: "398", volume: 1000000 },
        { date: "2026-03-02", close: "402", volume: 1100000 },
        { date: "2026-03-03", close: "405", volume: 1200000 },
        { date: "2026-03-04", close: "407", volume: 1300000 },
        { date: "2026-03-05", close: "410", volume: 1250000 },
      ],
      meta: meta(),
    });
  });

  await page.route("**/api/macro", async (route) =>
    routeJson(route, {
      indicators: [
        { symbol: "^GSPC", name: "S&P 500", value: "5100", change_pct: "0.8" },
        { symbol: "DX-Y.NYB", name: "DXY", value: "104.2", change_pct: "-0.2" },
      ],
      sections: [
        {
          id: "commodities",
          label: "Commodities",
          indicators: [{ symbol: "GC=F", name: "Gold", value: "2200", change_pct: "0.6" }],
        },
      ],
      top_movers: [
        { symbol: "^GSPC", name: "S&P 500", value: "5100", change_pct: "0.8" },
        { symbol: "DX-Y.NYB", name: "DXY", value: "104.2", change_pct: "-0.2" },
      ],
      market_breadth: {
        up: 2,
        down: 1,
        flat: 0,
        avg_change_pct: "0.4",
        strongest: { symbol: "^GSPC", name: "S&P 500", value: "5100", change_pct: "0.8" },
        weakest: { symbol: "DX-Y.NYB", name: "DXY", value: "104.2", change_pct: "-0.2" },
      },
      economy_snapshot: {
        bls_metrics: [
          { key: "CUUR0000SA0", label: "CPI (YoY index)", value: "313.8", date: "2026-03-01" },
          { key: "LNS14000000", label: "Unemployment Rate", value: "4.1", date: "2026-03-01" },
        ],
        sentiment: [
          { index_type: "crypto", value: 42, classification: "Fear", timestamp: 1762406400 },
          { index_type: "traditional", value: 58, classification: "Neutral", timestamp: 1762406400 },
        ],
        upcoming_events: [
          { date: "2026-03-06", name: "Non-Farm Payrolls", impact: "high", forecast: "180K" },
        ],
        predictions: [
          { question: "Fed cuts by June?", probability_pct: "61", volume_24h: "125000", category: "Econ" },
        ],
      },
      meta: meta(),
    }),
  );

  await page.route("**/api/transactions**", async (route) => {
    const method = route.request().method();
    const url = new URL(route.request().url());
    const parts = url.pathname.split("/").filter(Boolean);
    const id = Number(parts[parts.length - 1]);

    if (method === "POST" && parts[parts.length - 1] === "transactions") {
      const body = route.request().postDataJSON() as {
        symbol?: string;
        category?: string;
        tx_type?: string;
        quantity?: string;
        price_per?: string;
        currency?: string;
        date?: string;
        notes?: string | null;
      } | null;
      const symbol = (body?.symbol || "").trim().toUpperCase();
      if (!symbol) return routeJson(route, { ok: false, id: null, action: "noop" });
      transactions.unshift({
        id: nextTxId++,
        symbol,
        category: body?.category || "equity",
        tx_type: body?.tx_type || "buy",
        quantity: body?.quantity || "0",
        price_per: body?.price_per || "0",
        currency: body?.currency || "USD",
        date: body?.date || "2026-03-01",
        notes: body?.notes || null,
      });
      return routeJson(route, { ok: true, id: transactions[0].id, action: "created" });
    }

    if (method === "PATCH") {
      const body = route.request().postDataJSON() as {
        symbol?: string;
        category?: string;
        tx_type?: string;
        quantity?: string;
        price_per?: string;
        currency?: string;
        date?: string;
        notes?: string | null;
      } | null;
      const tx = transactions.find((t) => t.id === id);
      if (!tx) return routeJson(route, { ok: false, id, action: "noop" });
      tx.symbol = (body?.symbol || tx.symbol).toUpperCase();
      tx.category = body?.category || tx.category;
      tx.tx_type = body?.tx_type || tx.tx_type;
      tx.quantity = body?.quantity || tx.quantity;
      tx.price_per = body?.price_per || tx.price_per;
      tx.currency = body?.currency || tx.currency;
      tx.date = body?.date || tx.date;
      tx.notes = body?.notes || null;
      return routeJson(route, { ok: true, id, action: "updated" });
    }

    if (method === "DELETE") {
      const idx = transactions.findIndex((t) => t.id === id);
      if (idx >= 0) transactions.splice(idx, 1);
      return routeJson(route, { ok: idx >= 0, id, action: idx >= 0 ? "removed" : "noop" });
    }

    return routeJson(route, {
      transactions,
      sort_by: "date",
      sort_order: "desc",
      meta: meta(),
    });
  });

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

  await page.route("**/api/alerts**", async (route) => {
    const method = route.request().method();
    const url = new URL(route.request().url());
    const parts = url.pathname.split("/").filter(Boolean);
    const id = Number(parts[parts.length - 1]);
    const action = parts[parts.length - 1];
    const action2 = parts[parts.length - 2];

    if (method === "POST" && parts[parts.length - 1] === "alerts") {
      const body = route.request().postDataJSON() as { rule_text?: string } | null;
      const rule = (body?.rule_text || "").trim();
      if (!rule) return routeJson(route, { ok: false, id: null, action: "noop" });
      const tokens = rule.split(/\s+/);
      const symbol = (tokens[0] || "UNKNOWN").toUpperCase();
      const direction = (tokens[1] || "above").toLowerCase();
      const threshold = tokens[2] || "0";
      alerts.unshift({
        id: nextAlertId++,
        kind: "price",
        symbol,
        direction,
        threshold,
        rule_text: `${symbol} ${direction} ${threshold}`,
        status: "armed",
        triggered_at: null,
      });
      return routeJson(route, { ok: true, id: alerts[0].id, action: "created" });
    }

    if (method === "DELETE" && action2 === "alerts" && Number.isFinite(id)) {
      const idx = alerts.findIndex((a) => a.id === id);
      if (idx >= 0) alerts.splice(idx, 1);
      return routeJson(route, { ok: idx >= 0, id, action: idx >= 0 ? "removed" : "noop" });
    }

    if (method === "POST" && parts[parts.length - 1] === "ack") {
      const ackId = Number(parts[parts.length - 2]);
      const alert = alerts.find((a) => a.id === ackId);
      const ok = Boolean(alert && alert.status === "triggered");
      if (alert && ok) alert.status = "acknowledged";
      return routeJson(route, { ok, id: ackId, action: "acknowledged" });
    }

    if (method === "POST" && action === "rearm") {
      const rearmId = Number(parts[parts.length - 2]);
      const alert = alerts.find((a) => a.id === rearmId);
      const ok = Boolean(alert && (alert.status === "triggered" || alert.status === "acknowledged"));
      if (alert && ok) {
        alert.status = "armed";
        alert.triggered_at = null;
      }
      return routeJson(route, { ok, id: rearmId, action: "rearmed" });
    }

    return routeJson(route, { alerts, meta: meta() });
  });

  await page.route("**/api/news**", async (route) => {
    const url = new URL(route.request().url());
    const q = (url.searchParams.get("search") || "").toLowerCase();
    const source = (url.searchParams.get("source") || "").toLowerCase();
    const category = (url.searchParams.get("category") || "").toLowerCase();
    const all = [
      {
        id: 1,
        title: "Apple gains on strong iPhone demand",
        url: "https://example.com/apple",
        source: "Reuters",
        category: "markets",
        published_at: 1772726400,
        fetched_at: "2026-03-05T19:55:00Z",
      },
      {
        id: 2,
        title: "Fed officials signal caution on early cuts",
        url: "https://example.com/fed",
        source: "Bloomberg",
        category: "macro",
        published_at: 1772812800,
        fetched_at: "2026-03-05T20:05:00Z",
      },
      {
        id: 3,
        title: "Bitcoin options skew flips toward calls",
        url: "https://example.com/btc",
        source: "CoinDesk",
        category: "crypto",
        published_at: 1772816400,
        fetched_at: "2026-03-05T20:10:00Z",
      },
    ];
    const entries = all
      .filter((item) => (q ? item.title.toLowerCase().includes(q) : true))
      .filter((item) => (source ? item.source.toLowerCase().includes(source) : true))
      .filter((item) => (category ? item.category === category : true));
    return routeJson(route, { entries, meta: meta() });
  });

  await page.route("**/api/journal**", async (route) => {
    const method = route.request().method();
    const url = new URL(route.request().url());
    const parts = url.pathname.split("/").filter(Boolean);

    if (method === "POST" && parts[parts.length - 1] === "journal") {
      const body = route.request().postDataJSON() as {
        content?: string;
        symbol?: string | null;
        tag?: string | null;
        status?: string | null;
      } | null;
      const content = (body?.content || "").trim();
      if (!content) return routeJson(route, { ok: false, id: null, action: "noop" });
      journalEntries.unshift({
        id: nextJournalId++,
        symbol: body?.symbol || null,
        status: body?.status || "open",
        tag: body?.tag || null,
        conviction: null,
        content,
        timestamp: "2026-03-05T20:00:00Z",
      });
      return routeJson(route, { ok: true, id: journalEntries[0].id, action: "created" });
    }

    if (method === "PATCH") {
      const id = Number(parts[parts.length - 1]);
      const body = route.request().postDataJSON() as {
        content?: string;
        status?: string;
      } | null;
      const entry = journalEntries.find((j) => j.id === id);
      if (!entry) return routeJson(route, { ok: false, id, action: "noop" });
      if (typeof body?.content === "string" && body.content.trim()) entry.content = body.content.trim();
      if (typeof body?.status === "string" && body.status.trim()) entry.status = body.status.trim();
      return routeJson(route, { ok: true, id, action: "updated" });
    }

    if (method === "DELETE") {
      const id = Number(parts[parts.length - 1]);
      const idx = journalEntries.findIndex((j) => j.id === id);
      if (idx >= 0) journalEntries.splice(idx, 1);
      return routeJson(route, { ok: idx >= 0, id, action: idx >= 0 ? "removed" : "noop" });
    }

    const q = (url.searchParams.get("search") || "").toLowerCase();
    const entries = q
      ? journalEntries.filter((j) => (j.content || "").toLowerCase().includes(q))
      : journalEntries;
    return routeJson(route, { entries, meta: meta() });
  });

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
