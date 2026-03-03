#!/usr/bin/env bash
set -euo pipefail

REPO="skylarsimoncelli/pftui"
BINARY="pftui"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  os="linux" ;;
  Darwin) os="macos" ;;
  *) echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)  arch="x86_64" ;;
  aarch64|arm64) arch="aarch64" ;;
  *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

ASSET="${BINARY}-${arch}-${os}"

# Check if upgrading
CURRENT_VERSION=""
if command -v "$BINARY" &>/dev/null; then
  CURRENT_VERSION=$("$BINARY" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || echo "unknown")
  echo "🦀 Upgrading pftui (current: v${CURRENT_VERSION})..."
else
  echo "🦀 Installing pftui..."
fi
echo "   Platform: ${os}/${arch}"

# Get latest release tag
TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
echo "   Latest:   ${TAG}"

# Skip if already up to date
if [ -n "$CURRENT_VERSION" ] && [ "v${CURRENT_VERSION}" = "$TAG" ]; then
  echo ""
  echo "✅ Already up to date (${TAG})"
  exit 0
fi

# Download binary
URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"
echo "   Downloading ${ASSET}..."

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

if ! curl -fsSL "$URL" -o "${TMPDIR}/${BINARY}"; then
  echo ""
  echo "❌ Download failed. Binary not available for ${os}/${arch}."
  echo ""
  echo "Alternative install methods:"
  echo "  cargo install pftui          # Rust users"
  echo "  brew install skylarsimoncelli/tap/pftui  # macOS"
  echo "  docker run -it ghcr.io/skylarsimoncelli/pftui  # Docker"
  echo ""
  echo "See https://github.com/${REPO}#installation for all options."
  exit 1
fi

# Install binary (only replaces the binary — config and data are untouched)
# Data: ~/.local/share/pftui/pftui.db  — NOT affected
# Config: ~/.config/pftui/config.toml  — NOT affected
if [ -w "$INSTALL_DIR" ]; then
  mv "${TMPDIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
else
  echo "   Installing to ${INSTALL_DIR} (requires sudo)..."
  sudo mv "${TMPDIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
fi

chmod +x "${INSTALL_DIR}/${BINARY}"

echo ""
if [ -n "$CURRENT_VERSION" ]; then
  echo "✅ pftui upgraded: v${CURRENT_VERSION} → ${TAG}"
  echo "   Your portfolio data and config are unchanged."
else
  echo "✅ pftui ${TAG} installed to ${INSTALL_DIR}/${BINARY}"
  echo ""
  echo "Get started:"
  echo "  pftui          # launch TUI (setup wizard on first run)"
  echo "  pftui demo     # try with sample portfolio"
  echo "  pftui --help   # see all commands"
fi
