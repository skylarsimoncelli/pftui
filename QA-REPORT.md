# QA Report — pftui 2026-03-06

## Summary
- Tests run: 52 (manual CLI tests + edge cases + data integrity checks)
- Bugs found: 12 (P0: 2, P1: 5, P2: 5)
- Test suite: 1105 passing, 0 failing
- Clippy: clean (0 warnings)
- Release build: clean

## P0 — Critical Bugs (crashes, data corruption, wrong calculations)

### P0-1: `brief` and `movers` report wildly different 1D change percentages for the same assets

**Reproduction:**
```bash
pftui brief 2>&1 | grep -A6 "Top Movers"
pftui movers --threshold 0 2>&1 | grep -E "BTC|SI=F|GC=F"
```

**Observed:**
- `brief` reports BTC **-6.4%**, SI=F **+3.5%**, GC=F **+2.0%** for "1D" change
- `movers` reports BTC **-0.14%**, SI=F **-0.24%**, GC=F **-0.07%** for "1D" change
- Same assets, same prices, same timestamp — completely different percentages

**Root cause (likely):** `brief` appears to use Yahoo Finance's `regularMarketChangePercent` (actual trading day change from market open/previous close), while `movers` compares the last two cached price_history entries. After multiple refreshes in the same day, the "previous" cached entry is just the prior refresh (minutes ago), not yesterday's close.

**Impact:** Users cannot trust any "1D change" number. The two primary commands for checking daily performance contradict each other. This is the most critical bug — it undermines data trust across the entire tool.

### P0-2: `drift` command displays raw unformatted Decimal values with 30+ decimal places

**Reproduction:**
```bash
pftui target set GC=F --target 25
pftui target set BTC --target 20
pftui drift
```

**Observed:**
```
BTC    20   18.718814357195681326649469110   -1.28   2   ✓ In range
GC=F   25   25.440431019923431248529342220    0.44   2   ✓ In range
```

**Expected:** `Actual %` column should display `18.72` not `18.718814357195681326649469110`.

**Also affects:** `summary --json` outputs `allocation_pct` as raw high-precision strings (e.g. `"0.1304577185075538537229309600"`). The JSON should round to reasonable precision (2-4 decimal places).

## P1 — Significant Bugs (wrong output, missing data, poor error handling)

### P1-1: `pftui refresh` — 3 of 10 data sources consistently fail

**Reproduction:**
```bash
pftui refresh
```

**Observed:**
- `✗ COT (all failed)` — every refresh
- `✗ BLS (failed: Failed to parse BLS value: -)` — every refresh
- `✗ On-chain (failed: error decoding response body)` — every refresh
- ETF flows also returns no data (`pftui etf-flows` → "No ETF flow data available")

**Impact:** 4 of the headline "P0 Free Data Integration" features don't actually work. Features are marked ✅ COMPLETE in TODO.md but fail in production. Sentiment COT section shows "⚠️ unavailable" for all 4 assets.

### P1-2: `pftui global` shows empty data for all 8 countries despite "120 records" cached

**Reproduction:**
```bash
pftui global
```

**Observed:** All 8 country sections are completely empty — just headers with no data. Yet `pftui status` reports World Bank as "120 records, ✓ Fresh". The data is cached but not displayed.

### P1-3: `pftui status` shows inconsistent/misleading freshness for COMEX

**Reproduction:**
```bash
pftui status   # Shows COMEX: "never, 0 records, ✗ Empty"
pftui supply   # Shows actual COMEX data with gold and silver inventory
```

**Impact:** Status command reports COMEX as empty when data actually exists and is displayed by `pftui supply`.

### P1-4: COMEX registered inventory shows 0 troy ounces for both gold and silver

**Reproduction:**
```bash
pftui supply
```

**Observed:**
```
Gold: Registered: 0 troy ounces | Eligible: 2,015,405 troy ounces | Reg Ratio: 0.0%
Silver: Registered: 0 troy ounces | Eligible: 25,754,215 troy ounces | Reg Ratio: 0.0%
```

**Expected:** COMEX registered gold inventory should be ~16-18M troy ounces, not 0. The scraper appears to be parsing the wrong field or the source data structure changed.

### P1-5: USD/JPY and USD/CNY show 1.0000 in macro dashboard

**Reproduction:**
```bash
pftui macro | grep -E "JPY|CNY"
```

**Observed:** Both show `1.0000 → 0.00%`. Real values should be ~150 (JPY) and ~7.2 (CNY).

**Note:** This is already documented in TODO.md as a known issue with Yahoo Finance FX feed.

## P2 — Minor Issues (cosmetic, UX friction, edge cases)

### P2-1: `brief --technicals` flag doesn't exist (README implies it does)

**Reproduction:**
```bash
pftui brief --technicals
# error: unexpected argument '--technicals' found
```

**Note:** `brief` already includes a Technicals section by default. The flag from the test plan doesn't exist, but the README's CLI examples show `pftui brief` without `--technicals`. The brief help only shows `--json`. Not a bug per se, but the test plan expected it.

### P2-2: `add-tx` accepts zero quantity and zero price without validation

**Reproduction:**
```bash
pftui add-tx --symbol TEST --category equity --tx-type buy --quantity 0 --price 100 --date 2026-03-06
# "Added transaction #18: buy 0 TEST @ 100"

pftui add-tx --symbol TEST --category equity --tx-type buy --quantity 5 --price 0 --date 2026-03-06
# "Added transaction #19: buy 5 TEST @ 0"
```

**Expected:** Should reject quantity=0 and price=0 for buy transactions. A zero-quantity buy is meaningless; a zero-price buy corrupts cost basis.

### P2-3: `watch` accepts any arbitrary string without validation

**Reproduction:**
```bash
pftui watch INVALIDSYMBOL123
# "Added INVALIDSYMBOL123 (INVALIDSYMBOL123) to watchlist as equity"
pftui watchlist | grep INVALID
# Shows "N/A" for price — no error, no warning
```

**Expected:** At minimum, warn the user that the symbol couldn't be validated. Ideally, attempt a price lookup and warn if it fails.

### P2-4: No rate limiting on concurrent `pftui refresh`

**Reproduction:**
```bash
pftui refresh & sleep 2 && pftui refresh; wait
```

**Observed:** Both refreshes run fully in parallel, hitting all external APIs twice. No lock file or deduplication. Could lead to rate limiting from Yahoo Finance or other sources.

### P2-5: Performance command shows N/A for MTD despite having data from March 4

**Reproduction:**
```bash
pftui performance
# MTD: N/A / N/A (but we have snapshots from March 4, 5, 6)
```

**Expected:** MTD should be calculable since we have snapshots from earlier this month (March 4 is within the current month). Similarly QTD (Q1 started Jan 1 — but earliest snapshot is March 4, so N/A is debatable). The logic likely requires a snapshot from the first of the period.

## Passing Tests (commands that worked correctly)

### Core Commands ✅
- `pftui summary` — correct positions, allocation, gain/loss, technicals
- `pftui value` — correct total with gain/loss and category breakdown
- `pftui brief` — well-formatted markdown output with all sections (but 1D % discrepancy — see P0-1)
- `pftui brief --json` — valid structured JSON
- `pftui list-tx` — all 10 transactions listed correctly
- `pftui export json` — valid JSON, passes `python3 -m json.tool`
- `pftui export csv` — correct CSV with all positions
- `pftui watchlist` — 34 symbols with prices, change %, and freshness
- `pftui refresh` — fetches prices for 51 symbols, predictions, news, sentiment, calendar, COMEX
- `pftui macro` — complete dashboard with yields, currencies, commodities, volatility, derived metrics
- `pftui movers` — correctly filters by threshold, shows source
- `pftui movers --threshold 1` — 14 movers shown
- `pftui performance` — shows 1D and since-inception returns
- `pftui drift` — shows drift vs targets (formatting bug noted)
- `pftui history --date 2026-03-04` — correct historical snapshot
- `pftui snapshot` — full TUI render to stdout with ANSI codes
- `pftui alerts list/add/remove` — full CRUD works
- `pftui demo` — launches with temp DB
- `pftui predictions` — shows Polymarket data (50 items)
- `pftui news` — 20 articles from Bloomberg feeds
- `pftui sentiment` — Fear & Greed indices displayed (COT unavailable)
- `pftui supply` — COMEX data displayed (accuracy questionable)
- `pftui status` — shows freshness for all 10 sources
- `pftui journal list` — shows journal entries
- `pftui target set/list/remove` — full target management
- `pftui rebalance` — correct trade suggestions

### Edge Cases ✅
- `pftui movers --threshold 0` — shows all 38 symbols (correct)
- `pftui movers --threshold 100` — graceful "No movers" message
- `pftui history --date 2099-01-01` — "Date is in the future" error (correct)
- `pftui history --date 2020-01-01` — shows only cash position (correct, no other assets held then)
- `pftui export json | python3 -m json.tool` — valid JSON confirmed
- `pftui set-cash USD -- -100` — rejects negative amount (correct)
- `pftui set-cash USD 0` — clears position (correct)
- `pftui remove-tx 9999` — "Transaction #9999 not found" error (correct)
- `pftui add-tx` (no args) — prompts for symbol then errors (acceptable)

### Data Integrity ✅
- Allocations sum to exactly 100.00% across all positions
- `pftui value` and `pftui brief` report consistent total ($363,356.35)
- Export JSON positions match summary output
- Macro dashboard has no N/A or NaN values (except known USD/JPY, USD/CNY bug)

### Build & Tests ✅
- `cargo test` — **1105 tests passing, 0 failing**
- `cargo clippy --all-targets -- -D warnings` — **clean, 0 warnings**
- `cargo build --release` — **clean build in 21s**
