# API Sources — Free, No Key Required

> Every data source pftui integrates must work out-of-the-box with zero configuration.
> No API keys. No sign-up. No rate limit registration. Install and go.
> Sources requiring free keys are documented separately as optional enhancements.

## Tier 1: Core Integration (P0)

### Polymarket Gamma API
- **URL:** `https://gamma-api.polymarket.com/`
- **Auth:** None
- **Rate limit:** Generous (undocumented, no issues at reasonable polling)
- **Data:** Prediction market prices (= probabilities), volume, categories, market metadata
- **Endpoints:**
  - `GET /markets` — list all markets, filter by tag/category
  - `GET /markets/{id}` — single market detail
  - Response includes `outcomePrices` (array of floats, sum to 1.0 = probabilities)
- **Update cadence:** Real-time (prices update continuously)
- **pftui integration:** F17 (Economy tab predictions panel, CLI, sparklines)

### CFTC Commitments of Traders (Socrata API)
- **URL:** `https://publicreporting.cftc.gov/resource/`
- **Auth:** None (Socrata open data)
- **Rate limit:** 1000 rows per request, throttled at high volume
- **Data:** Futures positioning by trader type for all CFTC-reported contracts
- **Endpoints:**
  - Disaggregated Futures: `/72hh-t7tf.json`
  - Legacy Futures: `/6dca-aqww.json`
  - TFF (Financial Futures): `/gpe5-46if.json`
  - Filter: `?$where=market_and_exchange_names like '%GOLD%'&$order=report_date_as_yyyy_mm_dd DESC&$limit=12`
- **Key fields:** `noncomm_positions_long_all`, `noncomm_positions_short_all`, `comm_positions_long_all`, `comm_positions_short_all`, `open_interest_all`
- **Update cadence:** Weekly (Friday 3:30 PM ET, data as of prior Tuesday)
- **Contract mapping:** GC=Gold, SI=Silver, CL=Crude Oil, NG=Nat Gas, HG=Copper, BTC=Bitcoin (CME)
- **pftui integration:** F18 (asset detail COT panel, Markets tab summary, CLI)

### Alternative.me Crypto Fear & Greed
- **URL:** `https://api.alternative.me/fng/`
- **Auth:** None
- **Rate limit:** Reasonable (no documented limit)
- **Data:** Crypto F&G index (0-100), classification, timestamp
- **Endpoints:**
  - `GET /fng/` — current value
  - `GET /fng/?limit=30` — last 30 days
  - `GET /fng/?date_format=us` — US date format
- **Update cadence:** Daily
- **pftui integration:** F19 (header gauge, Economy tab sparkline, CLI)

### RSS Feeds (Multiple Sources)
- **Auth:** None
- **Rate limit:** None (standard HTTP polling)
- **Default feed list:**

| Source | Feed URL | Category |
|--------|----------|----------|
| Reuters Business | `https://www.reutersagency.com/feed/` | Macro |
| CoinDesk | `https://www.coindesk.com/arc/outboundfeeds/rss/` | Crypto |
| ZeroHedge | `https://cms.zerohedge.com/fullrss2.xml` | Macro/Alt |
| The Block | `https://www.theblock.co/rss.xml` | Crypto |
| Yahoo Finance | `https://finance.yahoo.com/news/rssindex` | General |
| MarketWatch | `http://feeds.marketwatch.com/marketwatch/topstories/` | General |
| CNBC | `https://search.cnbc.com/rs/search/combinedcms/view.xml?partnerId=wrss01&id=100003114` | General |
| Kitco Gold | `https://www.kitco.com/rss/` | Commodities |

- **Update cadence:** Poll every 10 minutes
- **pftui integration:** F20 (News tab, header ticker, per-asset news, CLI)

### Blockchair (Bitcoin On-Chain)
- **URL:** `https://api.blockchair.com/bitcoin/`
- **Auth:** None
- **Rate limit:** 5 requests/second (no key), reduced data without key
- **Data:** Block data, transaction data, address data, stats
- **Endpoints:**
  - `GET /bitcoin/stats` — network stats, mempool, hashrate
  - `GET /bitcoin/transactions?q=output_total(1000000000..)` — large transactions (>10 BTC)
- **Update cadence:** Real-time
- **pftui integration:** F21 (BTC detail popup, whale alerts)

### CoinGlass (BTC ETF Flows — Scrape)
- **URL:** `https://www.coinglass.com/etf/bitcoin`
- **Auth:** None (public page)
- **Data:** Daily net ETF inflows by fund (IBIT, FBTC, GBTC, etc.), cumulative flows
- **Method:** Scrape public page or intercept API calls
- **Update cadence:** Daily (after market close)
- **pftui integration:** F21 (BTC detail popup ETF flows, CLI)

### CME Group (COMEX Data — Scrape)
- **URL:** `https://www.cmegroup.com/daily_bulletin/`
- **Auth:** None (public pages)
- **Data:** COMEX gold/silver registered + eligible inventory, delivery notices, volume, open interest
- **Method:** Scrape daily bulletin PDFs or HTML pages
- **Update cadence:** Daily
- **pftui integration:** F22 (metals detail popup, CLI)

### World Gold Council (GoldHub — Scrape)
- **URL:** `https://www.gold.org/goldhub/data/`
- **Auth:** None (public data pages)
- **Data:** Central bank gold purchases, gold ETF flows, demand/supply statistics
- **Update cadence:** Monthly (CB data), weekly (ETF flows)
- **pftui integration:** F22 (gold detail popup, CLI)

### BLS API v1 (No Registration)
- **URL:** `https://api.bls.gov/publicAPI/v1/timeseries/data/`
- **Auth:** None for v1
- **Rate limit:** 10 queries/day, 10 years max, 25 series per query
- **Key series IDs:**
  - `CUUR0000SA0` — CPI-U (All Urban Consumers)
  - `LNS14000000` — Unemployment Rate
  - `CES0000000001` — Total Nonfarm Payrolls
  - `CES0500000003` — Average Hourly Earnings
  - `JTS000000000000000JOL` — JOLTS Job Openings
- **Update cadence:** Monthly (cache aggressively — 10 calls/day is tight)
- **pftui integration:** F24 (Economy tab live indicators)

### World Bank Open Data API
- **URL:** `https://api.worldbank.org/v2/`
- **Auth:** None
- **Rate limit:** Unlimited
- **Data:** 200+ countries, thousands of indicators
- **Endpoints:**
  - `GET /country/US;CN;IN;RU;BR;ZA;GB/indicator/NY.GDP.MKTP.KD.ZG?format=json` — GDP growth
  - `GET /country/US/indicator/GC.DOD.TOTL.GD.ZS?format=json` — Debt/GDP
  - `GET /country/all/indicator/FI.RES.TOTL.CD?format=json` — Reserves
- **Key indicators:**
  - `NY.GDP.MKTP.KD.ZG` — GDP growth (%)
  - `GC.DOD.TOTL.GD.ZS` — Central govt debt (% of GDP)
  - `BN.CAB.XOKA.GD.ZS` — Current account balance (% of GDP)
  - `FI.RES.TOTL.CD` — Total reserves (USD)
  - `FP.CPI.TOTL.ZG` — Inflation, consumer prices (%)
- **Update cadence:** Quarterly/Annual (cache heavily)
- **pftui integration:** F25 (Economy tab global macro panel, CLI)

### TradingEconomics Calendar (Scrape)
- **URL:** `https://tradingeconomics.com/calendar`
- **Auth:** None (public page)
- **Data:** Upcoming economic releases with date, event, forecast, previous, actual, impact level
- **Method:** Scrape public calendar page
- **Update cadence:** Daily
- **pftui integration:** F23 (Economy tab calendar, header countdown)

## Tier 2: Optional Enhancements (Free Key Required)

> These require a free registration + API key. Documented for users who want to opt-in.
> pftui should gracefully degrade when these keys are not configured.

| API | Free Tier | Key for |
|-----|-----------|---------|
| Finnhub | 60/min | Earnings calendar, company news, economic calendar |
| Alpha Vantage | 25/day | Pre-computed technicals, news sentiment |
| FRED | 120/min | 840k economic time series (upgrades BLS v1) |
| FMP | 250/day | Social sentiment per ticker, institutional holders |
| Twelve Data | 800/day | OHLCV candles, WebSocket streaming |
| Marketaux | 100/day | Sentiment-scored news per ticker |
| BLS v2 | 500/day | More calls, longer history than v1 |

**Config pattern:** `~/.config/pftui/keys.toml`
```toml
# Optional API keys — pftui works without any of these
[keys]
# finnhub = "your_key"
# alpha_vantage = "your_key"
# fred = "your_key"
```
