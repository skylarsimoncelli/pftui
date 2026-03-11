#!/usr/bin/env bash
set -euo pipefail

cmd_json_or_null() {
  local cmd="$1"
  local out
  if out=$(eval "$cmd" 2>/dev/null); then
    if [ -n "$out" ]; then
      printf '%s' "$out"
      return
    fi
  fi
  printf 'null'
}

cat <<JSON
{
  "generated_at_utc": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "summary": $(cmd_json_or_null "pftui analytics summary --json"),
  "low": $(cmd_json_or_null "pftui analytics low --json"),
  "medium": $(cmd_json_or_null "pftui analytics medium --json"),
  "high": $(cmd_json_or_null "pftui analytics high --json"),
  "macro": $(cmd_json_or_null "pftui analytics macro --json"),
  "brief": $(cmd_json_or_null "pftui brief --json"),
  "convictions": $(cmd_json_or_null "pftui conviction list --json"),
  "scenarios": $(cmd_json_or_null "pftui scenario list --json"),
  "trends": $(cmd_json_or_null "pftui trends list --json"),
  "predictions": $(cmd_json_or_null "pftui predict list --json"),
  "regime": $(cmd_json_or_null "pftui regime current --json")
}
JSON
