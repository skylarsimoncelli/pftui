#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 <version> <sha256>"
  echo "Example: $0 0.1.2 abc123..."
  exit 1
fi

VERSION="$1"
SHA256="$2"
OUT_DIR="packaging/aur"
OUT_FILE="${OUT_DIR}/PKGBUILD"

mkdir -p "$OUT_DIR"

cat > "$OUT_FILE" <<EOF
# Maintainer: Skylar Simoncelli <skylarsimoncelli@gmail.com>
pkgname=pftui
pkgver=${VERSION}
pkgrel=1
pkgdesc="Terminal portfolio tracker with real-time prices and charts"
arch=('x86_64')
url="https://github.com/skylarsimoncelli/pftui"
license=('MIT')
depends=('glibc' 'gcc-libs')
source=("\${pkgname}-\${pkgver}-x86_64::https://github.com/skylarsimoncelli/pftui/releases/download/v\${pkgver}/pftui-x86_64-linux")
sha256sums=('${SHA256}')

package() {
  install -Dm755 "\${srcdir}/\${pkgname}-\${pkgver}-x86_64" "\${pkgdir}/usr/bin/pftui"
}
EOF

echo "Rendered $OUT_FILE for v$VERSION"
