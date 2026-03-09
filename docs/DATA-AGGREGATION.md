# Data Aggregation Engine

## Purpose

The Data Aggregation Engine is pftui's ingestion layer. It pulls market, macro, sentiment, and event data from many sources and normalizes everything into one local database you own.

One command:

```bash
pftui refresh
```

This updates your prices, history, macro inputs, sentiment, prediction markets, news, and event data in a single pipeline.

## Why It Matters

- Eliminates multi-tab data scavenging.
- Gives agents one stable interface instead of many brittle scrapers.
- Produces a proprietary dataset that compounds with each refresh.
- Keeps control local: SQLite or your own PostgreSQL.

## Sources Unified

Core built-in sources include:

- Yahoo Finance (equities, ETFs, FX, commodities)
- CoinGecko (crypto)
- Polymarket (prediction markets)
- CFTC/Socrata (COT)
- Alternative.me (Fear & Greed)
- BLS (US economic series)
- World Bank (global macro)
- CME/COMEX (warehouse data)
- RSS feeds (market headlines)

Optional: Brave Search API for richer research/news workflows.

## Pipeline Shape

`refresh` runs in staged passes:

1. FX + spot prices
2. Price history updates/backfill
3. Correlation/regime snapshots
4. Predictions/news/calendar/sentiment/macro datasets
5. Snapshot + alert evaluation

This gives downstream layers (database, analytics, AI) consistent, timestamped input.

## Output Contract

The aggregation engine writes normalized records into your configured backend:

- `sqlite` (default)
- `postgres` (fully supported)

Both backends expose the same command/API semantics through backend dispatch.

## Operator Notes

- Run `pftui refresh` before analysis commands.
- For automation, schedule at least 2 runs/day (pre-open and after close).
- Use `pftui status --json` to inspect freshness across source groups.
