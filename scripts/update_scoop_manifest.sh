#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 <version> <sha256>"
  echo "Example: $0 0.1.2 abc123..."
  exit 1
fi

VERSION="$1"
SHA256="$2"
MANIFEST="scoop/pftui.json"

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required"
  exit 1
fi

if [[ ! -f "$MANIFEST" ]]; then
  echo "Missing $MANIFEST"
  exit 1
fi

URL="https://github.com/skylarsimoncelli/pftui/releases/download/v${VERSION}/pftui-x86_64-windows.exe"

tmp="$(mktemp)"
jq \
  --arg version "$VERSION" \
  --arg url "$URL" \
  --arg hash "$SHA256" \
  '.version=$version
   | .architecture."64bit".url=$url
   | .architecture."64bit".hash=$hash
   | .autoupdate.architecture."64bit".url="https://github.com/skylarsimoncelli/pftui/releases/download/v$version/pftui-x86_64-windows.exe"' \
  "$MANIFEST" > "$tmp"
mv "$tmp" "$MANIFEST"

echo "Updated $MANIFEST for v$VERSION"
