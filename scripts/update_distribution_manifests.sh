#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 ]]; then
  echo "Usage: $0 <version> <tarball_sha256> <windows_exe_sha256>" >&2
  echo "Example: $0 0.6.0 <sha256_tarball> <sha256_windows_exe>" >&2
  exit 1
fi

version="$1"
tar_sha="$2"
win_sha="$3"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# Homebrew formula
sed -i.bak -E "s#(url \"https://github.com/skylarsimoncelli/pftui/archive/refs/tags/v)[0-9.]+(\.tar\.gz\")#\\1${version}\\2#" Formula/pftui.rb
sed -i.bak -E "s#(sha256 \"[a-f0-9]{64}\")#sha256 \"${tar_sha}\"#" Formula/pftui.rb
rm -f Formula/pftui.rb.bak

# Snap
sed -i.bak -E "s#^version: '.*'#version: '${version}'#" snap/snapcraft.yaml
rm -f snap/snapcraft.yaml.bak

# Scoop
sed -i.bak -E "s#\"version\": \"[0-9.]+\"#\"version\": \"${version}\"#" scoop/pftui.json
sed -i.bak -E "s#(releases/download/v)[0-9.]+(/pftui-x86_64-windows\.exe)#\\1${version}\\2#" scoop/pftui.json
sed -i.bak -E "s#\"hash\": \"[A-Fa-f0-9]*\"#\"hash\": \"${win_sha}\"#" scoop/pftui.json
rm -f scoop/pftui.json.bak

echo "Updated distribution manifests to v${version}."
