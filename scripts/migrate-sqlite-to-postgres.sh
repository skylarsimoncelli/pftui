#!/bin/bash
# Migrate data from SQLite to PostgreSQL native tables
# Run once after F32 native Postgres is in place

set -euo pipefail

SQLITE_DB="$HOME/.local/share/pftui/pftui.db"
PG_CONN="postgres://pftui:pftui_sentinel_2026@127.0.0.1:5432/pftui"
PSQL="psql -U pftui -d pftui -h 127.0.0.1"
export PGPASSWORD="pftui_sentinel_2026"

echo "=== SQLite → PostgreSQL Data Migration ==="
echo "Source: $SQLITE_DB"
echo "Target: $PG_CONN"
echo ""

# Check source exists
if [ ! -f "$SQLITE_DB" ]; then
    echo "ERROR: SQLite database not found at $SQLITE_DB"
    exit 1
fi

# Function to migrate a table
migrate_table() {
    local table=$1
    local insert_sql=$2
    local count_before
    local count_after
    
    count_before=$($PSQL -t -c "SELECT count(*) FROM $table;" 2>/dev/null | tr -d ' ')
    
    if [ "$count_before" -gt 0 ]; then
        echo "  ⚠️  $table: already has $count_before rows — SKIPPING"
        return
    fi
    
    echo -n "  Migrating $table... "
    $PSQL -c "$insert_sql" > /dev/null 2>&1 || true
    
    # Use COPY approach via temp file
    count_after=$($PSQL -t -c "SELECT count(*) FROM $table;" 2>/dev/null | tr -d ' ')
    echo "$count_after rows"
}

# ---- TRANSACTIONS ----
echo "Transactions..."
sqlite3 "$SQLITE_DB" -csv "SELECT symbol, category, tx_type, quantity, price_per, currency, date, notes FROM transactions;" | \
    $PSQL -c "COPY transactions(symbol, category, tx_type, quantity, price_per, currency, date, notes) FROM STDIN WITH CSV;" 2>/dev/null && \
    echo "  ✅ transactions: $(sqlite3 "$SQLITE_DB" "SELECT count(*) FROM transactions;") rows" || \
    echo "  ❌ transactions failed"

# ---- WATCHLIST ----
echo "Watchlist..."
TX_COUNT=$($PSQL -t -c "SELECT count(*) FROM watchlist;" | tr -d ' ')
if [ "$TX_COUNT" -gt 0 ]; then
    echo "  ⚠️  watchlist already has $TX_COUNT rows — SKIPPING"
else
    sqlite3 "$SQLITE_DB" -csv "SELECT symbol, category, group_id, target_price, target_direction FROM watchlist;" | \
        $PSQL -c "COPY watchlist(symbol, category, group_id, target_price, target_direction) FROM STDIN WITH CSV;" 2>/dev/null && \
        echo "  ✅ watchlist: $(sqlite3 "$SQLITE_DB" "SELECT count(*) FROM watchlist;") rows" || \
        echo "  ❌ watchlist failed"
fi

# ---- ALERTS ----
echo "Alerts..."
TX_COUNT=$($PSQL -t -c "SELECT count(*) FROM alerts;" | tr -d ' ')
if [ "$TX_COUNT" -gt 0 ]; then
    echo "  ⚠️  alerts already has $TX_COUNT rows — SKIPPING"
else
    sqlite3 "$SQLITE_DB" -csv "SELECT kind, symbol, direction, threshold, status, rule_text FROM alerts WHERE status = 'armed';" | \
        $PSQL -c "COPY alerts(kind, symbol, direction, threshold, status, rule_text) FROM STDIN WITH CSV;" 2>/dev/null && \
        echo "  ✅ alerts (armed): $($PSQL -t -c "SELECT count(*) FROM alerts;" | tr -d ' ') rows" || \
        echo "  ❌ alerts failed"
fi

# ---- ALLOCATION TARGETS ----
echo "Allocation targets..."
TX_COUNT=$($PSQL -t -c "SELECT count(*) FROM allocation_targets;" | tr -d ' ')
if [ "$TX_COUNT" -gt 0 ]; then
    echo "  ⚠️  allocation_targets already has $TX_COUNT rows — SKIPPING"
else
    sqlite3 "$SQLITE_DB" -csv "SELECT symbol, target_pct, drift_band_pct FROM allocation_targets;" | \
        $PSQL -c "COPY allocation_targets(symbol, target_pct, drift_band_pct) FROM STDIN WITH CSV;" 2>/dev/null && \
        echo "  ✅ allocation_targets: $(sqlite3 "$SQLITE_DB" "SELECT count(*) FROM allocation_targets;") rows" || \
        echo "  ❌ allocation_targets failed"
fi

# ---- JOURNAL ----
echo "Journal..."
TX_COUNT=$($PSQL -t -c "SELECT count(*) FROM journal;" | tr -d ' ')
if [ "$TX_COUNT" -gt 0 ]; then
    echo "  ⚠️  journal already has $TX_COUNT rows — SKIPPING"
else
    sqlite3 "$SQLITE_DB" -csv "SELECT timestamp, content, tag, symbol, conviction, status FROM journal;" | \
        $PSQL -c "COPY journal(timestamp, content, tag, symbol, conviction, status) FROM STDIN WITH CSV;" 2>/dev/null && \
        echo "  ✅ journal: $(sqlite3 "$SQLITE_DB" "SELECT count(*) FROM journal;") rows" || \
        echo "  ❌ journal failed"
fi

echo ""
echo "=== Migration Complete ==="
echo ""
echo "Verify with: pftui db-info"
