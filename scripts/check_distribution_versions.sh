#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

cargo_version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
if [[ -z "$cargo_version" ]]; then
  echo "Failed to parse Cargo.toml version" >&2
  exit 1
fi

formula_version=$(sed -n 's/.*tags\/v\([0-9][0-9.]*\)\.tar\.gz.*/\1/p' Formula/pftui.rb | head -n1)
snap_version=$(sed -n "s/^version: '\(.*\)'/\1/p" snap/snapcraft.yaml | head -n1)
scoop_version=$(sed -n 's/.*"version": "\([^"]*\)".*/\1/p' scoop/pftui.json | head -n1)

fail=0
for pair in "Formula:$formula_version" "snap:$snap_version" "scoop:$scoop_version"; do
  name="${pair%%:*}"
  ver="${pair#*:}"
  if [[ -z "$ver" ]]; then
    echo "[dist-check] Could not parse version for $name" >&2
    fail=1
    continue
  fi
  if [[ "$ver" != "$cargo_version" ]]; then
    echo "[dist-check] Version mismatch: Cargo=$cargo_version, $name=$ver" >&2
    fail=1
  fi
done

if [[ "$fail" -ne 0 ]]; then
  echo "[dist-check] Run: scripts/update_distribution_manifests.sh $cargo_version <tarball_sha256> <windows_exe_sha256>" >&2
  exit 1
fi

echo "[dist-check] OK: distribution manifests match Cargo.toml version $cargo_version"
