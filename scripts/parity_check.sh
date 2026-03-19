#!/usr/bin/env bash
set -euo pipefail

PFTUI_BIN="${PFTUI_BIN:-pftui}"
POSTGRES_URL="${PFTUI_TEST_POSTGRES_URL:-${DATABASE_URL:-}}"

if ! command -v "$PFTUI_BIN" >/dev/null 2>&1; then
  echo "error: pftui binary not found. Set PFTUI_BIN or install pftui." >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required for normalized JSON comparison." >&2
  exit 1
fi

if [[ -z "$POSTGRES_URL" ]]; then
  echo "error: set PFTUI_TEST_POSTGRES_URL (or DATABASE_URL) for postgres parity checks." >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

sqlite_cfg_home="$tmp_dir/sqlite_cfg"
sqlite_data_home="$tmp_dir/sqlite_data"
pg_cfg_home="$tmp_dir/pg_cfg"
pg_data_home="$tmp_dir/pg_data"
mkdir -p "$sqlite_cfg_home/pftui" "$sqlite_data_home/pftui" "$pg_cfg_home/pftui" "$pg_data_home/pftui"

snapshot_json="$tmp_dir/snapshot.json"
cat >"$snapshot_json" <<'JSON'
{
  "config": { "base_currency": "USD", "portfolio_mode": "full", "theme": "default" },
  "transactions": [
    {
      "symbol": "AAPL",
      "category": "equity",
      "tx_type": "buy",
      "quantity": "10",
      "price_per": "150",
      "currency": "USD",
      "date": "2026-01-01",
      "notes": "parity-check"
    },
    {
      "symbol": "BTC",
      "category": "crypto",
      "tx_type": "buy",
      "quantity": "0.25",
      "price_per": "45000",
      "currency": "USD",
      "date": "2026-01-02",
      "notes": "parity-check"
    }
  ],
  "allocations": [],
  "watchlist": [
    { "symbol": "ETH", "category": "crypto" },
    { "symbol": "GLD", "category": "commodity" }
  ],
  "positions": []
}
JSON

sqlite_config="$sqlite_cfg_home/pftui/config.toml"
cat >"$sqlite_config" <<'TOML'
database_backend = "sqlite"
base_currency = "USD"
portfolio_mode = "full"
theme = "default"
TOML

pg_config="$pg_cfg_home/pftui/config.toml"
cat >"$pg_config" <<TOML
database_backend = "postgres"
database_url = "${POSTGRES_URL}"
base_currency = "USD"
portfolio_mode = "full"
theme = "default"
TOML

run_in_env() {
  local cfg_home="$1"
  local data_home="$2"
  shift 2
  XDG_CONFIG_HOME="$cfg_home" XDG_DATA_HOME="$data_home" "$@"
}

normalize_json() {
  local in_file="$1"
  local out_file="$2"
  jq -S '
    walk(
      if type == "object" then
        del(.id, .created_at, .updated_at, .added_at, .fetched_at, .snapshot_at, .recorded_at, .timestamp)
      elif type == "array" and (length > 0) and (.[0] | type == "object") and (.[0] | has("symbol")) then
        sort_by(.symbol)
      else
        .
      end
    )
  ' "$in_file" >"$out_file"
}

# Clear stale Postgres state so both backends start from identical data
psql "$POSTGRES_URL" -q -c "TRUNCATE price_cache, price_history, allocation_targets CASCADE;" 2>/dev/null || true

run_in_env "$sqlite_cfg_home" "$sqlite_data_home" "$PFTUI_BIN" system import "$snapshot_json" --mode replace >/dev/null
run_in_env "$pg_cfg_home" "$pg_data_home" "$PFTUI_BIN" system import "$snapshot_json" --mode replace >/dev/null

commands=(
  "portfolio value --json"
  "portfolio summary --json"
  "portfolio watchlist --json"
  "portfolio drift --json"
)

failures=0
for cmd in "${commands[@]}"; do
  sqlite_raw="$tmp_dir/sqlite_$(echo "$cmd" | tr ' /' '__').json"
  pg_raw="$tmp_dir/pg_$(echo "$cmd" | tr ' /' '__').json"
  sqlite_norm="$sqlite_raw.norm"
  pg_norm="$pg_raw.norm"

  run_in_env "$sqlite_cfg_home" "$sqlite_data_home" "$PFTUI_BIN" $cmd >"$sqlite_raw"
  run_in_env "$pg_cfg_home" "$pg_data_home" "$PFTUI_BIN" $cmd >"$pg_raw"

  normalize_json "$sqlite_raw" "$sqlite_norm"
  normalize_json "$pg_raw" "$pg_norm"

  if ! diff -u "$sqlite_norm" "$pg_norm" >/dev/null; then
    echo "parity mismatch: $cmd"
    diff -u "$sqlite_norm" "$pg_norm" || true
    failures=$((failures + 1))
  else
    echo "parity ok: $cmd"
  fi
done

if [[ "$failures" -gt 0 ]]; then
  echo
  echo "parity check failed: ${failures} command(s) differ between sqlite and postgres"
  exit 1
fi

echo
echo "parity check passed: sqlite and postgres outputs match for selected commands"
