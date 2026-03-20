#!/bin/bash
# Database integrity watchdog for pftui
# Checks critical table row counts against minimum thresholds.
# Exits 0 if healthy, exits 1 with details if anomaly detected.
#
# Run hourly via cron. Alert pipeline picks up non-zero exit.

set -euo pipefail

DB_PORT="50498"
DB_HOST="127.0.0.1"
DB_USER="pftui"
DB_NAME="pftui"
export PGPASSWORD="Rd9H0B66q8zDf8r0aHBe14HdvY6Kj7oD0GgueEBQ"

PSQL="psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d $DB_NAME -t -A"

# Critical tables and their minimum expected row counts.
# If a count drops below the threshold, something destructive happened.
# Update these thresholds as the DB grows.
declare -A THRESHOLDS=(
  [transactions]=10
  [watchlist]=30
  [allocation_targets]=3
  [scenarios]=3
  [convictions]=50
  [user_predictions]=100
  [price_history]=10000
  [power_metrics_history]=800
  [journal]=15
  [daily_notes]=50
  [thesis]=5
  [structural_cycles]=4
)

FAILURES=0
REPORT=""

for TABLE in "${!THRESHOLDS[@]}"; do
  MIN=${THRESHOLDS[$TABLE]}
  COUNT=$($PSQL -c "SELECT count(*) FROM $TABLE;" 2>/dev/null || echo "ERROR")

  if [[ "$COUNT" == "ERROR" ]]; then
    REPORT+="❌ $TABLE: query failed (table missing?)\n"
    FAILURES=$((FAILURES + 1))
  elif [[ "$COUNT" -lt "$MIN" ]]; then
    REPORT+="🚨 $TABLE: $COUNT rows (minimum: $MIN)\n"
    FAILURES=$((FAILURES + 1))
  fi
done

# Also check daemon is running
DAEMON_STATUS=$(pftui system daemon status 2>&1 || true)
if ! echo "$DAEMON_STATUS" | grep -q "Daemon running"; then
  REPORT+="🚨 Daemon is NOT running\n"
  FAILURES=$((FAILURES + 1))
fi

# Check last backup is less than 7 hours old
LATEST_BACKUP=$(ls -t /backups/pftui_*.sql.gz 2>/dev/null | head -1)
if [[ -z "$LATEST_BACKUP" ]]; then
  REPORT+="🚨 No backups found in /backups/\n"
  FAILURES=$((FAILURES + 1))
else
  BACKUP_AGE=$(( ($(date +%s) - $(stat -c %Y "$LATEST_BACKUP")) / 3600 ))
  if [[ "$BACKUP_AGE" -gt 7 ]]; then
    REPORT+="⚠️ Latest backup is ${BACKUP_AGE}h old (expected <7h): $LATEST_BACKUP\n"
    FAILURES=$((FAILURES + 1))
  fi
fi

if [[ "$FAILURES" -gt 0 ]]; then
  echo "=== DB WATCHDOG: $FAILURES ANOMALIES DETECTED ==="
  echo -e "$REPORT"
  exit 1
else
  echo "DB watchdog: all checks passed ($(date -u +%Y-%m-%dT%H:%M:%SZ))"
  exit 0
fi
