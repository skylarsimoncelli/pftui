#!/bin/bash
# Backup integrity verification for pftui
# Restores the latest backup to pftui_test DB and verifies row counts.
# Run daily (not hourly — it's heavier).

set -euo pipefail

DB_PORT="50498"
DB_HOST="127.0.0.1"
DB_USER="pftui"
DB_NAME_TEST="pftui_test"
export PGPASSWORD="Rd9H0B66q8zDf8r0aHBe14HdvY6Kj7oD0GgueEBQ"

PSQL_PROD="psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d pftui -t -A"
PSQL_TEST="psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d $DB_NAME_TEST -t -A"

LATEST_BACKUP=$(ls -t /backups/pftui_*.sql.gz 2>/dev/null | head -1)
if [[ -z "$LATEST_BACKUP" ]]; then
  echo "❌ No backup files found"
  exit 1
fi

echo "Verifying: $LATEST_BACKUP"

# Test gzip integrity first (fast)
if ! gzip -t "$LATEST_BACKUP" 2>/dev/null; then
  echo "❌ Backup file is corrupt (gzip integrity check failed)"
  exit 1
fi
echo "✅ gzip integrity OK"

# Create test DB if needed, then wipe and restore
psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d postgres -c "CREATE DATABASE $DB_NAME_TEST OWNER $DB_USER;" 2>/dev/null || true
psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d $DB_NAME_TEST -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;" >/dev/null 2>&1

gunzip -c "$LATEST_BACKUP" | psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d $DB_NAME_TEST -q >/dev/null 2>&1

# Compare critical table counts
CRITICAL_TABLES="transactions watchlist allocation_targets scenarios convictions user_predictions price_history journal daily_notes thesis"

FAILURES=0
echo ""
echo "=== Row Count Comparison (prod vs restored backup) ==="
printf "%-30s %10s %10s %s\n" "TABLE" "PROD" "BACKUP" "STATUS"
printf "%-30s %10s %10s %s\n" "-----" "----" "------" "------"

for TABLE in $CRITICAL_TABLES; do
  PROD_COUNT=$($PSQL_PROD -c "SELECT count(*) FROM $TABLE;" 2>/dev/null || echo "ERR")
  TEST_COUNT=$($PSQL_TEST -c "SELECT count(*) FROM $TABLE;" 2>/dev/null || echo "ERR")

  if [[ "$TEST_COUNT" == "ERR" ]]; then
    STATUS="❌ missing in backup"
    FAILURES=$((FAILURES + 1))
  elif [[ "$PROD_COUNT" == "ERR" ]]; then
    STATUS="⚠️ missing in prod"
    FAILURES=$((FAILURES + 1))
  elif [[ "$TEST_COUNT" -eq "$PROD_COUNT" ]]; then
    STATUS="✅"
  elif [[ "$TEST_COUNT" -lt "$PROD_COUNT" ]]; then
    DIFF=$((PROD_COUNT - TEST_COUNT))
    STATUS="⚠️ backup is $DIFF rows behind (OK if recent writes)"
  else
    STATUS="✅ backup has more ($TEST_COUNT)"
  fi

  printf "%-30s %10s %10s %s\n" "$TABLE" "$PROD_COUNT" "$TEST_COUNT" "$STATUS"
done

# Cleanup test DB
psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d postgres -c "DROP DATABASE IF EXISTS $DB_NAME_TEST;" >/dev/null 2>&1

echo ""
if [[ "$FAILURES" -gt 0 ]]; then
  echo "❌ Backup verification found $FAILURES issue(s)"
  exit 1
else
  echo "✅ Backup verification passed — $(basename $LATEST_BACKUP) is restorable ($(date -u +%Y-%m-%dT%H:%M:%SZ))"
  exit 0
fi
