#!/usr/bin/env bash
set -euo pipefail

CHECKLIST_FILE="${CHECKLIST_FILE:-docs/WEB_PARITY_CHECKLIST.md}"

if [[ ! -f "$CHECKLIST_FILE" ]]; then
  echo "missing checklist file: $CHECKLIST_FILE" >&2
  exit 1
fi

if [[ "$#" -lt 1 ]]; then
  echo "usage: $0 <item-id> [item-id ...]" >&2
  exit 1
fi

missing=()
for id in "$@"; do
  if ! grep -Eq "^- \\[x\\] ${id}\\." "$CHECKLIST_FILE"; then
    missing+=("$id")
  fi
done

if [[ "${#missing[@]}" -gt 0 ]]; then
  echo "web parity checklist items are not complete: ${missing[*]}" >&2
  exit 1
fi

echo "web parity checklist gate passed for item ids: $*"
