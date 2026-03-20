#!/bin/bash
# Daily PostgreSQL backup for pftui
# Keeps last 7 backups. Run via cron daily.

set -euo pipefail

BACKUP_DIR="/backups"
DB_NAME="pftui"
DB_USER="pftui"
DB_HOST="127.0.0.1"
KEEP=28
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="$BACKUP_DIR/pftui_${TIMESTAMP}.sql.gz"

DB_PORT="50498"
export PGPASSWORD="Rd9H0B66q8zDf8r0aHBe14HdvY6Kj7oD0GgueEBQ"

# Dump and compress
pg_dump -U "$DB_USER" -h "$DB_HOST" -p "$DB_PORT" "$DB_NAME" | gzip > "$BACKUP_FILE"

# Verify
if [ -s "$BACKUP_FILE" ]; then
    SIZE=$(du -h "$BACKUP_FILE" | cut -f1)
    echo "✅ Backup created: $BACKUP_FILE ($SIZE)"
else
    echo "❌ Backup failed: empty file"
    rm -f "$BACKUP_FILE"
    exit 1
fi

# Rotate: keep only last $KEEP backups
cd "$BACKUP_DIR"
ls -t pftui_*.sql.gz 2>/dev/null | tail -n +$((KEEP + 1)) | while read OLD; do
    echo "🗑️  Removing old backup: $OLD"
    rm -f "$OLD"
done

REMAINING=$(ls -1 pftui_*.sql.gz 2>/dev/null | wc -l)
echo "📦 $REMAINING backups retained in $BACKUP_DIR"
