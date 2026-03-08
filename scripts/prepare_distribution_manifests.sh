#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 <version> <release_dir>"
  echo "Example: $0 0.1.2 ./release"
  exit 1
fi

VERSION="$1"
RELEASE_DIR="$2"

LINUX_SHA_FILE="${RELEASE_DIR}/pftui-x86_64-linux.sha256"
WINDOWS_SHA_FILE="${RELEASE_DIR}/pftui-x86_64-windows.exe.sha256"

if [[ ! -f "$LINUX_SHA_FILE" ]]; then
  echo "Missing $LINUX_SHA_FILE"
  exit 1
fi
if [[ ! -f "$WINDOWS_SHA_FILE" ]]; then
  echo "Missing $WINDOWS_SHA_FILE"
  exit 1
fi

LINUX_SHA="$(awk '{print $1}' "$LINUX_SHA_FILE")"
WINDOWS_SHA="$(awk '{print $1}' "$WINDOWS_SHA_FILE")"

scripts/render_aur_pkgbuild.sh "$VERSION" "$LINUX_SHA"
scripts/update_scoop_manifest.sh "$VERSION" "$WINDOWS_SHA"

echo "Distribution manifests prepared for v$VERSION"
